//! Explicit, bounded, offline byte-to-text conversion for user-authorized
//! documents. A narrow supported PDF subset is deliberate: lopdf's built-in
//! decompression is not output-size bounded; all Flate page streams are first
//! decoded using a strictly capped independent reader and then made plain.
use std::{
    collections::BTreeSet,
    io::{Cursor, Read},
    ops::Range,
};

use flate2::bufread::ZlibDecoder;
use lopdf::{Document, Object};
use quick_xml::{events::Event, Reader};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::{EvidenceError, EvidenceIndex, IngestOutcome, Result, SourceFormat, SourceInput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentFormat {
    Pdf,
    Docx,
}

/// This is a *caller assertion*, not cryptographic license verification.
/// User-owned documents, compatible licenses, and permitted quotations are
/// acceptable; this library does not grant copying or redistribution rights.
#[derive(Clone, Copy, Debug)]
pub struct DocumentRights<'a> {
    pub acknowledgment: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtractionLimits {
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_archive_entries: usize,
    pub max_pdf_objects: usize,
    pub max_pages: usize,
    pub max_paragraphs: usize,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 2 * 1024 * 1024,
            max_output_bytes: 512 * 1024,
            max_archive_entries: 128,
            max_pdf_objects: 512,
            max_pages: 64,
            max_paragraphs: 8_192,
        }
    }
}

impl ExtractionLimits {
    fn check(&self, index: &EvidenceIndex, bytes: &[u8]) -> Result<()> {
        if self.max_input_bytes == 0
            || self.max_input_bytes > 4 * 1024 * 1024
            || self.max_output_bytes == 0
            || self.max_output_bytes > index.limits().max_document_bytes
            || self.max_archive_entries == 0
            || self.max_archive_entries > 256
            || self.max_pdf_objects == 0
            || self.max_pdf_objects > 1024
            || self.max_pages == 0
            || self.max_pages > 128
            || self.max_paragraphs == 0
            || self.max_paragraphs > 16_384
        {
            return Err(EvidenceError::InvalidLimits);
        }
        if bytes.is_empty() {
            return Err(EvidenceError::InvalidDocument);
        }
        if bytes.len() > self.max_input_bytes {
            return Err(EvidenceError::ExtractionLimit);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocationKind {
    PdfPage(u32),
    DocxParagraph(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLocation {
    /// Half-open UTF-8 offsets in extracted *indexed* text.
    pub byte_range: Range<usize>,
    pub kind: LocationKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentProvenance {
    /// Digest of original binary input (distinct from the EvidenceIndex
    /// digest of extracted UTF-8 text), for source-file identity checking.
    pub original_sha256: String,
    pub locations: Vec<DocumentLocation>,
}

impl DocumentProvenance {
    /// A retrieval chunk can cross two locations. Return *all* intersecting
    /// anchors, never falsely attribute a multi-page quote to one page.
    pub fn locations_for(&self, range: Range<usize>) -> Vec<&DocumentLocation> {
        self.locations
            .iter()
            .filter(|anchor| {
                anchor.byte_range.start < range.end && anchor.byte_range.end > range.start
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentImport {
    pub outcome: IngestOutcome,
    pub provenance: DocumentProvenance,
}

pub(crate) fn ingest_document(
    index: &mut EvidenceIndex,
    source_id: &str,
    relative_path: &str,
    format: DocumentFormat,
    bytes: &[u8],
    rights: DocumentRights<'_>,
    limits: ExtractionLimits,
) -> Result<DocumentImport> {
    if rights.acknowledgment.trim().is_empty()
        || rights.acknowledgment.len() > 256
        || rights.acknowledgment.chars().any(char::is_control)
    {
        return Err(EvidenceError::MissingSourceRights);
    }
    limits.check(index, bytes)?;
    let mut extracted = Extracted::default();
    match format {
        DocumentFormat::Pdf => pdf_text(bytes, limits, &mut extracted)?,
        DocumentFormat::Docx => docx_text(bytes, limits, &mut extracted)?,
    }
    if extracted.text.trim().is_empty() {
        return Err(EvidenceError::EmptySource);
    }
    // A single EvidenceIndex.ingest call commits *only* after all parsing,
    // quotas, path/identity checks and chunk validation have succeeded.
    let outcome = index.ingest(SourceInput {
        source_id,
        relative_path,
        format: SourceFormat::PlainText,
        text: &extracted.text,
    })?;
    Ok(DocumentImport {
        outcome,
        provenance: DocumentProvenance {
            original_sha256: format!("{:x}", Sha256::digest(bytes)),
            locations: extracted.locations,
        },
    })
}

#[derive(Default)]
struct Extracted {
    text: String,
    locations: Vec<DocumentLocation>,
}

impl Extracted {
    fn push(&mut self, content: &str, kind: LocationKind, max: usize) -> Result<()> {
        let content = content.trim();
        if content.is_empty() {
            return Ok(());
        }
        let overhead = usize::from(!self.text.is_empty());
        if self
            .text
            .len()
            .saturating_add(content.len())
            .saturating_add(overhead)
            > max
        {
            return Err(EvidenceError::ExtractionLimit);
        }
        if overhead > 0 {
            self.text.push('\n');
        }
        let start = self.text.len();
        self.text.push_str(content);
        self.locations.push(DocumentLocation {
            byte_range: start..self.text.len(),
            kind,
        });
        Ok(())
    }
}

fn pdf_text(input: &[u8], limits: ExtractionLimits, output: &mut Extracted) -> Result<()> {
    if !input.starts_with(b"%PDF-") {
        return Err(EvidenceError::InvalidDocument);
    }
    // Object streams and XRef streams may be inflated in Document::load_mem
    // *before* post-parse validation. Forbid them up front. Only ordinary
    // page-content Flate streams are permitted; we will decode those with
    // a bounded reader *after* parsing and strip their filters before lopdf
    // extract_text is allowed to inspect any content.
    // PDF name hex escaping (for example /F#69lter and /Obj#53tm)
    // can otherwise hide dangerous stream types from the raw-byte gate.
    // Reject all escaped-name markers rather than attempt partial parsing.
    if pdf_has_escaped_name(input)
        || input.windows(7).any(|w| w == b"/ObjStm")
        || input.windows(8).any(|w| w == b"/Encrypt")
        || pdf_has_unsafe_xref(input)
    {
        return Err(EvidenceError::UnsupportedDocument);
    }
    let raw_filters = input.windows(7).filter(|w| *w == b"/Filter").count();
    let mut doc = Document::load_mem(input).map_err(|_| EvidenceError::InvalidDocument)?;
    if doc.is_encrypted() {
        return Err(EvidenceError::UnsupportedDocument);
    }
    if doc.objects.len() > limits.max_pdf_objects {
        return Err(EvidenceError::ExtractionLimit);
    }
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err(EvidenceError::EmptySource);
    }
    if pages.len() > limits.max_pages {
        return Err(EvidenceError::ExtractionLimit);
    }
    // Page content may have exactly one ordinary FlateDecode filter, no
    // predictors or indirect filter aliases. All other filtered streams,
    // including image/font/object/XRef streams, remain unsupported.
    let mut content_ids = BTreeSet::new();
    for page_id in pages.values() {
        content_ids.extend(doc.get_page_contents(*page_id));
    }
    let mut total_content = 0usize;
    let mut filtered_count = 0usize;
    let mut inflate = Vec::new();
    for (id, object) in &doc.objects {
        if let Object::Stream(stream) = object {
            if stream.dict.get(b"DecodeParms").is_ok()
                || stream.dict.get(b"Type").ok().and_then(|v| v.as_name().ok())
                    == Some(b"ObjStm".as_slice())
            {
                return Err(EvidenceError::UnsupportedDocument);
            }
            total_content = total_content.saturating_add(stream.content.len());
            if total_content > limits.max_input_bytes {
                return Err(EvidenceError::ExtractionLimit);
            }
            if let Ok(filter) = stream.dict.get(b"Filter") {
                filtered_count += 1;
                let standard_flate = match filter {
                    Object::Name(name) => name == b"FlateDecode",
                    Object::Array(filters) if filters.len() == 1 => {
                        matches!(&filters[0], Object::Name(name) if name == b"FlateDecode")
                    }
                    _ => false,
                };
                if !standard_flate || !content_ids.contains(id) {
                    return Err(EvidenceError::UnsupportedDocument);
                }
                inflate.push(*id);
            }
        }
    }
    // Includes disallowing /Filter tokens only in trailing garbage/comments,
    // or in nonstream dictionaries. Retains previous fail-closed behavior.
    if filtered_count != raw_filters {
        return Err(EvidenceError::UnsupportedDocument);
    }
    let mut total_expanded = 0usize;
    for id in inflate {
        let stream = doc
            .get_object_mut(id)
            .and_then(Object::as_stream_mut)
            .map_err(|_| EvidenceError::InvalidDocument)?;
        // Never use lopdf::Stream::decompressed_content here: it calls an
        // unbounded read_to_end and may expand a tiny bomb without limits.
        let remaining = limits.max_output_bytes.saturating_sub(total_expanded);
        let mut decoder = ZlibDecoder::new(stream.content.as_slice());
        let mut decoded = Vec::new();
        decoder
            .by_ref()
            .take(remaining as u64 + 1)
            .read_to_end(&mut decoded)
            .map_err(|_| EvidenceError::InvalidDocument)?;
        if decoded.len() > remaining {
            return Err(EvidenceError::ExtractionLimit);
        }
        // A complete zlib stream including its Adler32 checksum must be
        // consumed. Reject concatenation/trailing payload, not just prefixes.
        if decoder.total_in() as usize != stream.content.len() {
            return Err(EvidenceError::InvalidDocument);
        }
        total_expanded += decoded.len();
        stream.set_plain_content(decoded);
    }
    for page in pages.keys() {
        let content = doc
            .extract_text(&[*page])
            .map_err(|_| EvidenceError::InvalidDocument)?;
        output.push(
            &content,
            LocationKind::PdfPage(*page),
            limits.max_output_bytes,
        )?;
    }
    Ok(())
}

/// PDF names permit #xx escaping, which can conceal dangerous Filter or
/// ObjStm dictionary keys from our conservative byte-level gate.
/// A '#' in normal text or xref data does not trigger this check.
fn pdf_has_escaped_name(input: &[u8]) -> bool {
    for (offset, byte) in input.iter().enumerate() {
        if *byte != b'/' {
            continue;
        }
        let name = &input[offset + 1..];
        for &part in name {
            if matches!(
                part,
                b' ' | b'\t'
                    | b'\r'
                    | b'\n'
                    | b'\x0c'
                    | b'('
                    | b')'
                    | b'<'
                    | b'>'
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'/'
                    | b'%'
            ) {
                break;
            }
            if part == b'#' {
                return true;
            }
        }
    }
    false
}

/// lopdf may process cross-reference streams while loading the document.
/// Allow only the explicit, *unfiltered* XRef dictionary subset (used by
/// lopdf's own writer). Refuse filtered XRefs BEFORE Document::load_mem and
/// reject XRefStm hybrid references, malformed/oversized dictionaries, and
/// suspicious nested dictionary spans. This is deliberately conservative.
fn pdf_has_unsafe_xref(input: &[u8]) -> bool {
    for (offset, _) in input.windows(5).enumerate().filter(|(_, w)| *w == b"/XRef") {
        let suffix = &input[offset + 5..];
        if suffix.starts_with(b"Stm") {
            return true;
        }
        // Anything other than a PDF-name delimiter is an unrecognized name.
        if suffix
            .first()
            .is_some_and(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>' | b']'))
        {
            return true;
        }
        let preceding = &input[..offset];
        let Some(start) = preceding.windows(2).rposition(|part| part == b"<<") else {
            return true;
        };
        if preceding
            .windows(2)
            .rposition(|part| part == b">>")
            .is_some_and(|end| end > start)
        {
            return true;
        }
        let Some(end) = suffix.windows(2).position(|part| part == b">>") else {
            return true;
        };
        if offset + 5 + end + 2 - start > 4096 {
            return true;
        }
        let dictionary = &input[start..offset + 5 + end + 2];
        if dictionary.windows(7).any(|part| part == b"/Filter")
            || dictionary.windows(12).any(|part| part == b"/DecodeParms")
            || dictionary.windows(2).filter(|part| *part == b"<<").count() != 1
        {
            return true;
        }
    }
    false
}

fn docx_text(input: &[u8], limits: ExtractionLimits, output: &mut Extracted) -> Result<()> {
    if !input.starts_with(b"PK\x03\x04") {
        return Err(EvidenceError::InvalidDocument);
    }
    let mut zip =
        ZipArchive::new(Cursor::new(input)).map_err(|_| EvidenceError::InvalidDocument)?;
    if zip.len() > limits.max_archive_entries {
        return Err(EvidenceError::ExtractionLimit);
    }
    let mut names = BTreeSet::new();
    let mut total_expanded = 0u64;
    let mut document_xml = None;
    let mut content_types = false;
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|_| EvidenceError::InvalidDocument)?;
        let name = entry.name().to_owned();
        if name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name.contains(':')
            || name.chars().any(char::is_control)
            || name
                .split('/')
                .any(|p| p == "." || p == ".." || p.is_empty())
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err(EvidenceError::InvalidDocument);
        }
        if let Some(mode) = entry.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(EvidenceError::UnsupportedDocument); // symlink
            }
        }
        if name.ends_with("vbaProject.bin") || name.starts_with("_xmlsignatures/") {
            return Err(EvidenceError::UnsupportedDocument);
        }
        if !matches!(
            entry.compression(),
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
        ) {
            return Err(EvidenceError::UnsupportedDocument);
        }
        total_expanded = total_expanded.saturating_add(entry.size());
        if total_expanded > limits.max_input_bytes as u64 * 2
            || entry.size() > limits.max_output_bytes as u64 * 2
        {
            return Err(EvidenceError::ExtractionLimit);
        }
        if name == "[Content_Types].xml" {
            content_types = true;
        }
        if name == "word/document.xml" {
            let mut buf = Vec::new();
            entry
                .take(
                    (limits.max_output_bytes as u64)
                        .saturating_mul(2)
                        .saturating_add(1),
                )
                .read_to_end(&mut buf)
                .map_err(|_| EvidenceError::InvalidDocument)?;
            if buf.len() > limits.max_output_bytes.saturating_mul(2) {
                return Err(EvidenceError::ExtractionLimit);
            }
            document_xml = Some(buf);
        }
    }
    let xml = document_xml.ok_or(EvidenceError::InvalidDocument)?;
    if !content_types {
        return Err(EvidenceError::InvalidDocument);
    }
    // Avoid resolving external entities. Reader does not perform filesystem
    // or network I/O; DTD and processing instructions are explicitly rejected.
    let mut parser = Reader::from_reader(xml.as_slice());
    parser.config_mut().trim_text(false);
    let mut paragraph = String::new();
    let mut inside_paragraph = false;
    let mut text_runs = 0usize;
    let mut paragraph_count = 0usize;
    let mut events = 0usize;
    loop {
        events += 1;
        if events > limits.max_input_bytes {
            return Err(EvidenceError::ExtractionLimit);
        }
        let event = parser
            .read_event()
            .map_err(|_| EvidenceError::InvalidDocument)?;
        match event {
            Event::Start(ref e) if e.name().as_ref() == b"w:p" => {
                if inside_paragraph {
                    return Err(EvidenceError::InvalidDocument);
                }
                inside_paragraph = true;
                paragraph.clear();
            }
            Event::Start(ref e) if e.name().as_ref() == b"w:t" && inside_paragraph => {
                text_runs += 1;
            }
            Event::End(ref e) if e.name().as_ref() == b"w:t" && inside_paragraph => {
                text_runs = text_runs.saturating_sub(1);
            }
            Event::Text(ref e) if inside_paragraph && text_runs > 0 => {
                let text = e.unescape().map_err(|_| EvidenceError::InvalidDocument)?;
                if paragraph.len().saturating_add(text.len()) > limits.max_output_bytes {
                    return Err(EvidenceError::ExtractionLimit);
                }
                paragraph.push_str(&text);
            }
            Event::Empty(ref e) if inside_paragraph && e.name().as_ref() == b"w:tab" => {
                paragraph.push(' ');
            }
            Event::Empty(ref e)
                if inside_paragraph && matches!(e.name().as_ref(), b"w:br" | b"w:cr") =>
            {
                paragraph.push(' ');
            }
            Event::End(ref e) if e.name().as_ref() == b"w:p" => {
                if !inside_paragraph || text_runs != 0 {
                    return Err(EvidenceError::InvalidDocument);
                }
                paragraph_count += 1;
                if paragraph_count > limits.max_paragraphs {
                    return Err(EvidenceError::ExtractionLimit);
                }
                output.push(
                    &paragraph,
                    LocationKind::DocxParagraph(paragraph_count as u32),
                    limits.max_output_bytes,
                )?;
                inside_paragraph = false;
            }
            Event::DocType(_) | Event::PI(_) | Event::CData(_) => {
                return Err(EvidenceError::UnsupportedDocument);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if inside_paragraph || text_runs != 0 {
        return Err(EvidenceError::InvalidDocument);
    }
    Ok(())
}
