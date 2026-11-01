//! Source-grounded, offline lexical retrieval over caller-provided UTF-8.
//!
//! This crate has no filesystem, network, model, embedding or generation API.
//! An evidence hit always contains an exact slice of its original source;
//! a score is only lexical relevance, never proof of factual correctness.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

mod documents;
pub use documents::{
    DocumentFormat, DocumentImport, DocumentLocation, DocumentProvenance, DocumentRights,
    ExtractionLimits, LocationKind,
};

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
const MAX_ID_BYTES: usize = 96;
const MAX_PATH_BYTES: usize = 1_024;
const MAX_UNIQUE_QUERY_TERMS: usize = 64;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    #[error("invalid index limits")]
    InvalidLimits,
    #[error("source identifier must be a safe nonempty ASCII identifier")]
    InvalidSourceId,
    #[error("source path must be a portable, normalized relative path")]
    InvalidSourcePath,
    #[error("source is empty or contains no searchable words")]
    EmptySource,
    #[error("source bytes are not valid UTF-8")]
    InvalidUtf8,
    #[error("a document with this source identifier already has different content or metadata")]
    ConflictingSourceId,
    #[error("a different source identifier already owns this path")]
    ConflictingPath,
    #[error("document count limit exceeded")]
    DocumentLimit,
    #[error("document byte limit exceeded")]
    DocumentByteLimit,
    #[error("total source byte limit exceeded")]
    TotalByteLimit,
    #[error("chunk count limit exceeded")]
    ChunkLimit,
    #[error("a character could not fit within the configured chunk byte limit")]
    ChunkTooSmall,
    #[error("query must contain searchable words and fit configured limits")]
    InvalidQuery,
    #[error("requested result count exceeds the configured limit")]
    InvalidTopK,
    #[error("unsupported or malformed document container")]
    InvalidDocument,
    #[error("document contains unsupported compression, encryption or active content")]
    UnsupportedDocument,
    #[error("document extraction exceeded configured byte, entry, object or page budget")]
    ExtractionLimit,
    #[error("document import requires an explicit rights declaration")]
    MissingSourceRights,
}

pub type Result<T> = std::result::Result<T, EvidenceError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFormat {
    PlainText,
    Markdown,
}

/// Only caller-provided content is indexed. A relative path is provenance
/// metadata, not a directive to read the filesystem.
#[derive(Clone, Copy, Debug)]
pub struct SourceInput<'a> {
    pub source_id: &'a str,
    pub relative_path: &'a str,
    pub format: SourceFormat,
    pub text: &'a str,
}

/// Hard, explicitly validated limits. All sizes are UTF-8 *bytes*, not chars.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexLimits {
    pub max_documents: usize,
    pub max_document_bytes: usize,
    pub max_total_bytes: usize,
    pub max_chunks: usize,
    pub max_chunk_bytes: usize,
    pub max_query_bytes: usize,
    pub max_top_k: usize,
}

impl Default for IndexLimits {
    fn default() -> Self {
        Self {
            max_documents: 128,
            max_document_bytes: 512 * 1024,
            max_total_bytes: 8 * 1024 * 1024,
            max_chunks: 16_384,
            max_chunk_bytes: 1_024,
            max_query_bytes: 2_048,
            max_top_k: 32,
        }
    }
}

impl IndexLimits {
    pub fn validate(&self) -> Result<()> {
        if self.max_documents == 0
            || self.max_documents > 10_000
            || self.max_document_bytes == 0
            || self.max_document_bytes > 8 * 1024 * 1024
            || self.max_total_bytes == 0
            || self.max_total_bytes > 64 * 1024 * 1024
            || self.max_document_bytes > self.max_total_bytes
            || self.max_chunks == 0
            || self.max_chunks > 100_000
            || self.max_chunk_bytes < 4
            || self.max_chunk_bytes > 16_384
            || self.max_chunk_bytes > self.max_document_bytes
            || self.max_query_bytes == 0
            || self.max_query_bytes > 16_384
            || self.max_top_k == 0
            || self.max_top_k > 1_000
        {
            Err(EvidenceError::InvalidLimits)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngestOutcome {
    Added { chunks: usize },
    AlreadyPresent,
}

/// Counts observed after an all-or-nothing refresh from a trusted caller's
/// *complete* source inventory. A source not supplied in that inventory is
/// removed. This is a bounded in-memory rebuild, not a filesystem watcher.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CorpusRefresh {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMetadata<'a> {
    pub source_id: &'a str,
    pub relative_path: &'a str,
    pub format: SourceFormat,
    pub sha256: &'a str,
    pub source_bytes: usize,
    pub chunk_count: usize,
}

/// An exact, independently checkable quote from one source.
///
/// byte_range.start and .end are offsets into the original UTF-8 source, and
/// quote == &source_text[start..end]. No text is generated or paraphrased.
#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceHit<'a> {
    pub source_id: &'a str,
    pub relative_path: &'a str,
    pub source_sha256: &'a str,
    pub format: SourceFormat,
    pub chunk_index: usize,
    pub byte_range: Range<usize>,
    pub quote: &'a str,
    pub matched_terms: Vec<String>,
    pub bm25_score: f64,
}

struct Source {
    id: String,
    path: String,
    path_key: String,
    format: SourceFormat,
    text: String,
    sha256: String,
    chunk_count: usize,
}

struct Chunk {
    source_idx: usize,
    source_chunk_index: usize,
    range: Range<usize>,
    tf: BTreeMap<String, usize>,
    tokens: usize,
}

/// Pure in-memory index; no persistence or hidden I/O.
pub struct EvidenceIndex {
    limits: IndexLimits,
    sources: Vec<Source>,
    chunks: Vec<Chunk>,
    total_bytes: usize,
    total_tokens: usize,
    doc_freq: BTreeMap<String, usize>,
}

impl EvidenceIndex {
    /// Extract a caller-supplied, rights-acknowledged PDF/DOCX and commit its
    /// normalized plain text to the existing deterministic index atomically.
    /// No file paths are opened; no network or model activity occurs.
    pub fn ingest_document(
        &mut self,
        source_id: &str,
        relative_path: &str,
        format: DocumentFormat,
        bytes: &[u8],
        rights: DocumentRights<'_>,
        limits: ExtractionLimits,
    ) -> Result<DocumentImport> {
        documents::ingest_document(
            self,
            source_id,
            relative_path,
            format,
            bytes,
            rights,
            limits,
        )
    }

    pub fn new(limits: IndexLimits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            limits,
            sources: Vec::new(),
            chunks: Vec::new(),
            total_bytes: 0,
            total_tokens: 0,
            doc_freq: BTreeMap::new(),
        })
    }

    pub fn limits(&self) -> IndexLimits {
        self.limits
    }

    pub fn document_count(&self) -> usize {
        self.sources.len()
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn total_source_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn source_text(&self, source_id: &str) -> Option<&str> {
        let key = source_id.to_ascii_lowercase();
        self.sources
            .iter()
            .find(|s| s.id.to_ascii_lowercase() == key)
            .map(|s| s.text.as_str())
    }

    pub fn source(&self, source_id: &str) -> Option<SourceMetadata<'_>> {
        let key = source_id.to_ascii_lowercase();
        self.sources
            .iter()
            .find(|s| s.id.to_ascii_lowercase() == key)
            .map(|s| SourceMetadata {
                source_id: &s.id,
                relative_path: &s.path,
                format: s.format,
                sha256: &s.sha256,
                source_bytes: s.text.len(),
                chunk_count: s.chunk_count,
            })
    }

    /// Refresh the complete corpus using caller-validated content. The staged
    /// index is fully validated before this instance changes, so a rejected
    /// input, duplicate identity, quota failure or invalid UTF-8 never drops
    /// existing citations. Removal and replacement rebuild BM25 statistics
    /// from the current sources rather than retaining stale document frequency.
    ///
    /// This performs no I/O and accepts no execution or network capabilities.
    /// The caller must explicitly provide *all* intended documents, including
    /// unchanged ones, and persist them externally if restart recovery matters.
    pub fn replace_corpus<'a>(
        &mut self,
        sources: impl IntoIterator<Item = SourceInput<'a>>,
    ) -> Result<CorpusRefresh> {
        let mut next = Self::new(self.limits)?;
        for input in sources {
            next.ingest(input)?;
        }

        let mut report = CorpusRefresh::default();
        for candidate in &next.sources {
            match self
                .sources
                .iter()
                .find(|existing| existing.id.eq_ignore_ascii_case(&candidate.id))
            {
                Some(previous)
                    if previous.path_key == candidate.path_key
                        && previous.format == candidate.format
                        && previous.sha256 == candidate.sha256 =>
                {
                    report.unchanged += 1;
                }
                Some(_) => report.updated += 1,
                None => report.added += 1,
            }
        }
        report.removed = self
            .sources
            .iter()
            .filter(|existing| {
                !next
                    .sources
                    .iter()
                    .any(|candidate| candidate.id.eq_ignore_ascii_case(&existing.id))
            })
            .count();
        *self = next;
        Ok(report)
    }

    /// Ingest caller-owned raw bytes, checking the byte cap *before* UTF-8
    /// decoding. This performs no file reads or hidden conversions.
    pub fn ingest_utf8_bytes(
        &mut self,
        source_id: &str,
        relative_path: &str,
        format: SourceFormat,
        bytes: &[u8],
    ) -> Result<IngestOutcome> {
        if bytes.len() > self.limits.max_document_bytes {
            return Err(EvidenceError::DocumentByteLimit);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| EvidenceError::InvalidUtf8)?;
        self.ingest(SourceInput {
            source_id,
            relative_path,
            format,
            text,
        })
    }

    /// Validate and atomically ingest exactly one caller-supplied source.
    /// Same ID+path+format+bytes is idempotent; conflicts fail closed.
    /// Identical content at different paths remains separate evidence.
    pub fn ingest(&mut self, input: SourceInput<'_>) -> Result<IngestOutcome> {
        validate_source_id(input.source_id)?;
        validate_relative_path(input.relative_path)?;
        if input.text.trim().is_empty() {
            return Err(EvidenceError::EmptySource);
        }
        if input.text.len() > self.limits.max_document_bytes {
            return Err(EvidenceError::DocumentByteLimit);
        }
        let path_key = normalized_path_key(input.relative_path);
        if let Some(previous) = self
            .sources
            .iter()
            .find(|s| s.id.eq_ignore_ascii_case(input.source_id))
        {
            return if previous.path_key == path_key
                && previous.format == input.format
                && previous.text == input.text
            {
                Ok(IngestOutcome::AlreadyPresent)
            } else {
                Err(EvidenceError::ConflictingSourceId)
            };
        }
        if self.sources.iter().any(|s| s.path_key == path_key) {
            return Err(EvidenceError::ConflictingPath);
        }
        if self.sources.len() >= self.limits.max_documents {
            return Err(EvidenceError::DocumentLimit);
        }
        if self.total_bytes.saturating_add(input.text.len()) > self.limits.max_total_bytes {
            return Err(EvidenceError::TotalByteLimit);
        }

        // Build and validate all chunks before mutating any index state.
        let raw_ranges = split_ranges(input.text, self.limits.max_chunk_bytes)?;
        if self.chunks.len().saturating_add(raw_ranges.len()) > self.limits.max_chunks {
            return Err(EvidenceError::ChunkLimit);
        }
        let mut pending = Vec::with_capacity(raw_ranges.len());
        for range in raw_ranges {
            let tf = term_frequencies(&input.text[range.clone()]);
            if !tf.is_empty() {
                let tokens = tf.values().sum();
                pending.push((range, tf, tokens));
            }
        }
        if pending.is_empty() {
            return Err(EvidenceError::EmptySource);
        }
        let source_idx = self.sources.len();
        let digest = format!("{:x}", Sha256::digest(input.text.as_bytes()));
        self.sources.push(Source {
            id: input.source_id.to_owned(),
            path: input.relative_path.to_owned(),
            path_key,
            format: input.format,
            text: input.text.to_owned(),
            sha256: digest,
            chunk_count: pending.len(),
        });
        for (index, (range, tf, tokens)) in pending.into_iter().enumerate() {
            for term in tf.keys() {
                *self.doc_freq.entry(term.clone()).or_insert(0) += 1;
            }
            self.total_tokens += tokens;
            self.chunks.push(Chunk {
                source_idx,
                source_chunk_index: index,
                range,
                tf,
                tokens,
            });
        }
        self.total_bytes += input.text.len();
        Ok(IngestOutcome::Added {
            chunks: self.sources[source_idx].chunk_count,
        })
    }

    /// BM25 over bounded, deterministic, Unicode-normalized chunk tokens.
    /// Returns only chunks with lexical matches; tie-breaks by source ID,
    /// relative path and byte offset independent of insertion order.
    pub fn search(&self, query: &str, top_k: usize) -> Result<Vec<EvidenceHit<'_>>> {
        if query.is_empty() || query.len() > self.limits.max_query_bytes {
            return Err(EvidenceError::InvalidQuery);
        }
        if top_k == 0 || top_k > self.limits.max_top_k {
            return Err(EvidenceError::InvalidTopK);
        }
        let query_terms: BTreeSet<String> = tokenize(query).into_iter().collect();
        if query_terms.is_empty() || query_terms.len() > MAX_UNIQUE_QUERY_TERMS {
            return Err(EvidenceError::InvalidQuery);
        }
        if self.chunks.is_empty() {
            return Ok(Vec::new());
        }

        let total_chunks = self.chunks.len() as f64;
        let average_length = self.total_tokens as f64 / total_chunks;
        let mut candidates = Vec::new();
        for chunk in &self.chunks {
            let mut score = 0.0;
            let mut matched_terms = Vec::new();
            for term in &query_terms {
                let Some(&frequency) = chunk.tf.get(term) else {
                    continue;
                };
                let df = *self
                    .doc_freq
                    .get(term)
                    .expect("indexed term has document frequency");
                let idf = (1.0 + (total_chunks - df as f64 + 0.5) / (df as f64 + 0.5)).ln();
                let f = frequency as f64;
                let normalization =
                    BM25_K1 * (1.0 - BM25_B + BM25_B * (chunk.tokens as f64 / average_length));
                score += idf * (f * (BM25_K1 + 1.0)) / (f + normalization);
                matched_terms.push(term.clone());
            }
            if matched_terms.is_empty() {
                continue;
            }
            let source = &self.sources[chunk.source_idx];
            let quote = &source.text[chunk.range.clone()];
            candidates.push(EvidenceHit {
                source_id: &source.id,
                relative_path: &source.path,
                source_sha256: &source.sha256,
                format: source.format,
                chunk_index: chunk.source_chunk_index,
                byte_range: chunk.range.clone(),
                quote,
                matched_terms,
                bm25_score: score,
            });
        }
        candidates.sort_by(|a, b| {
            b.bm25_score
                .total_cmp(&a.bm25_score)
                .then_with(|| a.source_id.cmp(b.source_id))
                .then_with(|| a.relative_path.cmp(b.relative_path))
                .then_with(|| a.byte_range.start.cmp(&b.byte_range.start))
                .then_with(|| a.byte_range.end.cmp(&b.byte_range.end))
        });
        candidates.truncate(top_k);
        Ok(candidates)
    }
}

fn normalized_path_key(path: &str) -> String {
    path.nfkc().flat_map(char::to_lowercase).collect()
}

fn validate_source_id(id: &str) -> Result<()> {
    if id.len() > MAX_ID_BYTES
        || id.is_empty()
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || id.contains("..")
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(EvidenceError::InvalidSourceId);
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.contains(['\\', ':', '\0'])
        || path.chars().any(char::is_control)
    {
        return Err(EvidenceError::InvalidSourcePath);
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
        {
            return Err(EvidenceError::InvalidSourcePath);
        }
        let basename = component
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || matches!(
                basename.as_str(),
                "COM1"
                    | "COM2"
                    | "COM3"
                    | "COM4"
                    | "COM5"
                    | "COM6"
                    | "COM7"
                    | "COM8"
                    | "COM9"
                    | "LPT1"
                    | "LPT2"
                    | "LPT3"
                    | "LPT4"
                    | "LPT5"
                    | "LPT6"
                    | "LPT7"
                    | "LPT8"
                    | "LPT9"
            )
        {
            return Err(EvidenceError::InvalidSourcePath);
        }
    }
    Ok(())
}

fn tokenize(text: &str) -> Vec<String> {
    // NFKC makes canonically/compatibility-equivalent forms searchable while
    // the quote remains a slice of the unmodified original UTF-8 source.
    let normalized: String = text.nfkc().flat_map(char::to_lowercase).collect();
    normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn term_frequencies(text: &str) -> BTreeMap<String, usize> {
    let mut tf = BTreeMap::new();
    for token in tokenize(text) {
        *tf.entry(token).or_insert(0) += 1;
    }
    tf
}

/// Partition a UTF-8 source into exact nonoverlapping spans. Prefer whitespace
/// boundaries, preserve original byte offsets, and always make forward progress
/// on scripts without whitespace or excessively long tokens.
fn split_ranges(text: &str, max_chunk_bytes: usize) -> Result<Vec<Range<usize>>> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        while cursor < text.len() {
            let c = text[cursor..]
                .chars()
                .next()
                .expect("cursor in UTF-8 string");
            if !c.is_whitespace() {
                break;
            }
            cursor += c.len_utf8();
        }
        if cursor == text.len() {
            break;
        }
        let mut hard_end = (cursor + max_chunk_bytes).min(text.len());
        while !text.is_char_boundary(hard_end) {
            hard_end -= 1;
        }
        if hard_end == cursor {
            return Err(EvidenceError::ChunkTooSmall);
        }
        let mut end = hard_end;
        if hard_end < text.len() {
            if let Some((relative, _)) = text[cursor..hard_end]
                .char_indices()
                .rev()
                .find(|(pos, character)| *pos > 0 && character.is_whitespace())
            {
                end = cursor + relative;
            }
        }
        let original_end = end;
        while end > cursor {
            let c = text[cursor..end]
                .chars()
                .next_back()
                .expect("nonempty span");
            if !c.is_whitespace() {
                break;
            }
            end -= c.len_utf8();
        }
        if end > cursor {
            spans.push(cursor..end);
        }
        cursor = original_end;
    }
    Ok(spans)
}
