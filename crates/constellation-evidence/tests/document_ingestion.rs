use std::io::{Cursor, Write};

use constellation_evidence::{
    DocumentFormat, DocumentRights, EvidenceError, EvidenceIndex, ExtractionLimits, IndexLimits,
    IngestOutcome, LocationKind,
};
use lopdf::{
    content::{Content, Operation},
    dictionary, Document, Object, Stream,
};
use sha2::{Digest, Sha256};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const RIGHTS: DocumentRights<'static> = DocumentRights {
    acknowledgment: "User-owned test fixture; indexing only",
};

fn index() -> EvidenceIndex {
    EvidenceIndex::new(IndexLimits::default()).unwrap()
}

fn pdf(pages: &[&str]) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resource_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font_id) },
    });
    let mut children = Vec::new();
    for text in pages {
        let instructions = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
                Operation::new("Td", vec![100.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(*text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let bytes = instructions.encode().unwrap();
        let stream_id = document.add_object(Stream::new(dictionary! {}, bytes));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Contents" => stream_id,
            "Resources" => resource_id,
        });
        children.push(page_id.into());
    }
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(children),
            "Count" => pages.len() as i64,
        }),
    );
    let catalog = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

fn docx_with_entries(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data, method) in entries {
        zip.start_file(
            *path,
            SimpleFileOptions::default().compression_method(*method),
        )
        .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn docx(xml: &str) -> Vec<u8> {
    docx_with_entries(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#,
            CompressionMethod::Deflated,
        ),
        (
            "word/document.xml",
            xml.as_bytes(),
            CompressionMethod::Deflated,
        ),
    ])
}

#[test]
fn real_uncompressed_pdf_import_preserves_exact_page_spans_and_search() {
    let pdf = pdf(&["Cortex alpha source", "Hippocampus memory encoding"]);
    let mut evidence = index();
    let imported = evidence
        .ingest_document(
            "paper-1",
            "papers/paper.pdf",
            DocumentFormat::Pdf,
            &pdf,
            RIGHTS,
            ExtractionLimits::default(),
        )
        .unwrap();
    assert!(matches!(imported.outcome, IngestOutcome::Added { .. }));
    assert_eq!(imported.provenance.locations.len(), 2);
    assert_eq!(
        imported.provenance.original_sha256,
        format!("{:x}", Sha256::digest(&pdf))
    );
    let text = evidence.source_text("paper-1").unwrap();
    assert!(text.contains("Cortex alpha source"));
    assert!(text.contains("Hippocampus memory encoding"));
    for (i, anchor) in imported.provenance.locations.iter().enumerate() {
        assert_eq!(anchor.kind, LocationKind::PdfPage((i + 1) as u32));
        assert!(!text[anchor.byte_range.clone()].is_empty());
    }
    let hits = evidence.search("Hippocampus", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(&text[hits[0].byte_range.clone()], hits[0].quote);
    assert_eq!(
        imported
            .provenance
            .locations_for(hits[0].byte_range.clone())
            .last()
            .unwrap()
            .kind,
        LocationKind::PdfPage(2)
    );
    assert_eq!(
        evidence
            .ingest_document(
                "paper-1",
                "papers/paper.pdf",
                DocumentFormat::Pdf,
                &pdf,
                RIGHTS,
                ExtractionLimits::default()
            )
            .unwrap()
            .outcome,
        IngestOutcome::AlreadyPresent
    );
}

#[test]
fn compressed_docx_import_decodes_xml_escapes_and_records_paragraphs() {
    let document = docx(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
          <w:body>
          <w:p><w:r><w:t>Neurons &amp; glia</w:t></w:r></w:p>
          <w:p><w:r><w:t>Hippocampus</w:t><w:tab/><w:t>memory</w:t></w:r></w:p>
          <w:p><w:r><w:t>UTF-8 فارسی</w:t></w:r></w:p>
          </w:body></w:document>"#,
    );
    let mut evidence = index();
    let imported = evidence
        .ingest_document(
            "doc-1",
            "papers/notes.docx",
            DocumentFormat::Docx,
            &document,
            RIGHTS,
            ExtractionLimits::default(),
        )
        .unwrap();
    let source = evidence.source_text("doc-1").unwrap();
    assert_eq!(source, "Neurons & glia\nHippocampus memory\nUTF-8 فارسی");
    assert_eq!(imported.provenance.locations.len(), 3);
    for (idx, anchor) in imported.provenance.locations.iter().enumerate() {
        assert_eq!(anchor.kind, LocationKind::DocxParagraph((idx + 1) as u32));
        assert_eq!(
            &source[anchor.byte_range.clone()],
            ["Neurons & glia", "Hippocampus memory", "UTF-8 فارسی"][idx]
        );
    }
    let results = evidence.search("Hippocampus", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert!(imported
        .provenance
        .locations_for(results[0].byte_range.clone())
        .iter()
        .any(|anchor| anchor.kind == LocationKind::DocxParagraph(2)));
}

#[test]
fn rejects_unacknowledged_rights_and_malformed_inputs_without_mutating_index() {
    let mut evidence = index();
    evidence
        .ingest_utf8_bytes(
            "prior",
            "prior.txt",
            constellation_evidence::SourceFormat::PlainText,
            b"original valid source",
        )
        .unwrap();
    let before = evidence.search("original", 4).unwrap()[0].quote.to_owned();
    let inputs = [
        (
            DocumentFormat::Pdf,
            b"%PDF-not really a document".as_slice(),
        ),
        (DocumentFormat::Pdf, b"<xml>not PDF</xml>".as_slice()),
        (DocumentFormat::Docx, b"PK\x03\x04garbage".as_slice()),
        (DocumentFormat::Docx, b"PDF is not a ZIP".as_slice()),
    ];
    for (format, bytes) in inputs {
        assert!(evidence
            .ingest_document(
                "bad",
                "bad.bin",
                format,
                bytes,
                RIGHTS,
                ExtractionLimits::default()
            )
            .is_err());
        assert_eq!(evidence.document_count(), 1);
    }
    assert_eq!(
        evidence
            .ingest_document(
                "bad",
                "bad.pdf",
                DocumentFormat::Pdf,
                &pdf(&["secret"]),
                DocumentRights {
                    acknowledgment: "  "
                },
                ExtractionLimits::default()
            )
            .unwrap_err(),
        EvidenceError::MissingSourceRights
    );
    assert_eq!(evidence.search("original", 4).unwrap()[0].quote, before);
}

#[test]
fn pdf_compression_and_object_streams_are_refused_before_parsing() {
    let mut evidence = index();
    let compact = pdf(&["one bounded page"]);
    for malicious in [
        b"/Filter /FlateDecode".as_slice(),
        b"/ObjStm".as_slice(),
        b"/Encrypt".as_slice(),
        b"/F#69lter".as_slice(),
    ] {
        let mut bytes = compact.clone();
        bytes.extend_from_slice(malicious);
        assert_eq!(
            evidence
                .ingest_document(
                    "bad",
                    "bad.pdf",
                    DocumentFormat::Pdf,
                    &bytes,
                    RIGHTS,
                    ExtractionLimits::default()
                )
                .unwrap_err(),
            EvidenceError::UnsupportedDocument
        );
    }
    assert_eq!(evidence.document_count(), 0);
}

#[test]
fn zip_bomb_output_limit_and_archive_traversal_are_refused() {
    let xml = format!(
        "<w:document xmlns:w=\"x\"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>",
        "x".repeat(120_000)
    );
    let big = docx(&xml);
    let mut evidence = index();
    let limits = ExtractionLimits {
        max_output_bytes: 2048,
        ..ExtractionLimits::default()
    };
    assert!(matches!(
        evidence.ingest_document(
            "big",
            "big.docx",
            DocumentFormat::Docx,
            &big,
            RIGHTS,
            limits
        ),
        Err(EvidenceError::ExtractionLimit)
    ));
    let traversal = docx_with_entries(&[
        (
            "[Content_Types].xml",
            b"<Types/>",
            CompressionMethod::Stored,
        ),
        ("../evil.xml", b"attack", CompressionMethod::Stored),
        (
            "word/document.xml",
            b"<w:document/>",
            CompressionMethod::Stored,
        ),
    ]);
    assert_eq!(
        evidence
            .ingest_document(
                "evil",
                "evil.docx",
                DocumentFormat::Docx,
                &traversal,
                RIGHTS,
                ExtractionLimits::default()
            )
            .unwrap_err(),
        EvidenceError::InvalidDocument
    );
    assert_eq!(evidence.document_count(), 0);
}

#[test]
fn archive_entry_count_and_windows_unsafe_names_fail_before_index_mutation() {
    let valid_xml = br#"<w:document><w:p><w:r><w:t>Valid words</w:t></w:r></w:p></w:document>"#;
    let many = docx_with_entries(&[
        (
            "[Content_Types].xml",
            b"<Types/>",
            CompressionMethod::Stored,
        ),
        ("word/document.xml", valid_xml, CompressionMethod::Deflated),
        ("extra.bin", b"unused", CompressionMethod::Stored),
    ]);
    let mut evidence = index();
    assert_eq!(
        evidence
            .ingest_document(
                "many",
                "many.docx",
                DocumentFormat::Docx,
                &many,
                RIGHTS,
                ExtractionLimits {
                    max_archive_entries: 2,
                    ..ExtractionLimits::default()
                },
            )
            .unwrap_err(),
        EvidenceError::ExtractionLimit
    );
    let colon = docx_with_entries(&[
        (
            "[Content_Types].xml",
            b"<Types/>",
            CompressionMethod::Stored,
        ),
        ("C:evil.xml", b"hidden", CompressionMethod::Stored),
        ("word/document.xml", valid_xml, CompressionMethod::Deflated),
    ]);
    assert_eq!(
        evidence
            .ingest_document(
                "unsafe",
                "unsafe.docx",
                DocumentFormat::Docx,
                &colon,
                RIGHTS,
                ExtractionLimits::default(),
            )
            .unwrap_err(),
        EvidenceError::InvalidDocument
    );
    assert_eq!(evidence.document_count(), 0);
}

#[test]
fn docx_dtd_external_entities_and_corrupt_markup_are_denied() {
    let bad = [
        r#"<!DOCTYPE a [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><w:p><w:t>&xxe;</w:t></w:p>"#,
        "<w:document><w:p><w:t>missing closes",
    ];
    for xml in bad {
        let mut evidence = index();
        assert!(evidence
            .ingest_document(
                "bad",
                "bad.docx",
                DocumentFormat::Docx,
                &docx(xml),
                RIGHTS,
                ExtractionLimits::default()
            )
            .is_err());
        assert_eq!(evidence.document_count(), 0);
    }
}

#[test]
fn quotas_for_page_count_source_and_extracted_text_are_atomic() {
    let mut evidence = index();
    let multi = pdf(&["Page one", "Page two", "Page three"]);
    let limits = ExtractionLimits {
        max_pages: 2,
        ..ExtractionLimits::default()
    };
    assert_eq!(
        evidence
            .ingest_document(
                "many",
                "many.pdf",
                DocumentFormat::Pdf,
                &multi,
                RIGHTS,
                limits
            )
            .unwrap_err(),
        EvidenceError::ExtractionLimit
    );
    let limits = ExtractionLimits {
        max_input_bytes: 32,
        ..ExtractionLimits::default()
    };
    assert_eq!(
        evidence
            .ingest_document(
                "large",
                "large.pdf",
                DocumentFormat::Pdf,
                &multi,
                RIGHTS,
                limits
            )
            .unwrap_err(),
        EvidenceError::ExtractionLimit
    );
    let limits = ExtractionLimits {
        max_output_bytes: 6,
        ..ExtractionLimits::default()
    };
    assert_eq!(
        evidence
            .ingest_document(
                "long",
                "long.pdf",
                DocumentFormat::Pdf,
                &multi,
                RIGHTS,
                limits
            )
            .unwrap_err(),
        EvidenceError::ExtractionLimit
    );
    assert_eq!(evidence.document_count(), 0);
}

#[test]
fn failed_conflicting_document_does_not_replace_original_evidence() {
    let mut evidence = index();
    let first = pdf(&["First original manuscript"]);
    let second = pdf(&["Second changed manuscript"]);
    evidence
        .ingest_document(
            "same",
            "same.pdf",
            DocumentFormat::Pdf,
            &first,
            RIGHTS,
            ExtractionLimits::default(),
        )
        .unwrap();
    let initial = evidence.source_text("same").unwrap().to_owned();
    assert_eq!(
        evidence
            .ingest_document(
                "same",
                "same.pdf",
                DocumentFormat::Pdf,
                &second,
                RIGHTS,
                ExtractionLimits::default()
            )
            .unwrap_err(),
        EvidenceError::ConflictingSourceId
    );
    assert_eq!(evidence.source_text("same"), Some(initial.as_str()));
    assert_eq!(evidence.search("original", 3).unwrap().len(), 1);
}
