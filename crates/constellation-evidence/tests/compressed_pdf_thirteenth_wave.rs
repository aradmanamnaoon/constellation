//! Bounded standard PDF Flate content extraction. These are actual serialized
//! PDFs, not a mock parser or hand-assembled text handed directly to the index.
use std::io::Write;

use constellation_evidence::{
    DocumentFormat, DocumentRights, EvidenceError, EvidenceIndex, ExtractionLimits, IndexLimits,
    LocationKind,
};
use flate2::{write::ZlibEncoder, Compression};
use lopdf::{
    content::{Content, Operation},
    dictionary, Document, Object, Stream,
};
use sha2::{Digest, Sha256};

const RIGHTS: DocumentRights<'static> = DocumentRights {
    acknowledgment: "This is a self-generated testing document and may be indexed.",
};

#[derive(Clone, Copy)]
enum Variant {
    Flate,
    FlateArray,
    Plain,
    BadChecksum,
    Truncated,
    Trailing,
    Predictor,
    Lzw,
    NestedFilters,
    BadFilter,
}

fn index() -> EvidenceIndex {
    EvidenceIndex::new(IndexLimits::default()).unwrap()
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut writer = ZlibEncoder::new(Vec::new(), Compression::default());
    writer.write_all(data).unwrap();
    writer.finish().unwrap()
}

fn pdf(pages: &[(&str, Variant)]) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font = document.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font) },
    });
    let mut children = Vec::new();
    for (text, variant) in pages {
        let instructions = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
                Operation::new("Td", vec![100.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(*text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let raw = instructions.encode().unwrap();
        let mut compressed = zlib(&raw);
        let mut dict = dictionary! {};
        match variant {
            Variant::Plain => compressed = raw,
            Variant::Flate => dict.set("Filter", Object::Name(b"FlateDecode".to_vec())),
            Variant::FlateArray => {
                dict.set(
                    "Filter",
                    Object::Array(vec![Object::Name(b"FlateDecode".to_vec())]),
                );
            }
            Variant::BadChecksum => {
                dict.set("Filter", "FlateDecode");
                *compressed.last_mut().unwrap() ^= 0x8f;
            }
            Variant::Truncated => {
                dict.set("Filter", "FlateDecode");
                compressed.truncate(compressed.len() - 3);
            }
            Variant::Trailing => {
                dict.set("Filter", "FlateDecode");
                compressed.extend_from_slice(b"TRAILING-INJECTION");
            }
            Variant::Predictor => {
                dict.set("Filter", "FlateDecode");
                dict.set("DecodeParms", dictionary! { "Predictor" => 12 });
            }
            Variant::Lzw => dict.set("Filter", "LZWDecode"),
            Variant::NestedFilters => {
                dict.set(
                    "Filter",
                    Object::Array(vec![
                        Object::Name(b"FlateDecode".to_vec()),
                        Object::Name(b"FlateDecode".to_vec()),
                    ]),
                );
            }
            Variant::BadFilter => dict.set("Filter", 13),
        }
        let contents = document.add_object(Stream::new(dict, compressed));
        let page = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Contents" => contents,
            "Resources" => resources,
        });
        children.push(page.into());
    }
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => Object::Array(children), "Count" => pages.len() as i64,
        }),
    );
    let root = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", root);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

fn ingest(bytes: &[u8], limits: ExtractionLimits) -> Result<(), EvidenceError> {
    let mut evidence = index();
    evidence.ingest_document(
        "paper",
        "paper.pdf",
        DocumentFormat::Pdf,
        bytes,
        RIGHTS,
        limits,
    )?;
    Ok(())
}

#[test]
fn multi_page_flate_and_filter_array_preserve_original_binary_sha_and_page_offsets() {
    let contents = pdf(&[
        ("Cortex alpha frequency", Variant::Flate),
        ("Hippocampus memory consolidation", Variant::FlateArray),
        ("Basal ganglia motor control", Variant::Plain),
    ]);
    let mut evidence = index();
    let imported = evidence
        .ingest_document(
            "compressed",
            "papers/compressed.pdf",
            DocumentFormat::Pdf,
            &contents,
            RIGHTS,
            ExtractionLimits::default(),
        )
        .unwrap();
    assert_eq!(
        imported.provenance.original_sha256,
        format!("{:x}", Sha256::digest(&contents))
    );
    let text = evidence.source_text("compressed").unwrap();
    assert_eq!(imported.provenance.locations.len(), 3);
    for (i, (expected, _)) in [
        ("Cortex alpha frequency", Variant::Flate),
        ("Hippocampus memory consolidation", Variant::FlateArray),
        ("Basal ganglia motor control", Variant::Plain),
    ]
    .iter()
    .enumerate()
    {
        let location = &imported.provenance.locations[i];
        assert_eq!(location.kind, LocationKind::PdfPage((i + 1) as u32));
        assert!(text[location.byte_range.clone()].contains(expected));
    }
    let hits = evidence.search("Hippocampus", 2).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(&text[hits[0].byte_range.clone()], hits[0].quote);
    assert!(imported
        .provenance
        .locations_for(hits[0].byte_range.clone())
        .iter()
        .any(|loc| loc.kind == LocationKind::PdfPage(2)));
}

#[test]
fn extreme_flate_expansion_and_cumulative_page_budget_refuse_without_partial_ingestion() {
    let mut evidence = index();
    let compressed = pdf(&[(&"H".repeat(180_000), Variant::Flate)]);
    assert!(
        compressed.len() < 5_000,
        "fixture should be a real extreme expansion"
    );
    assert_eq!(
        evidence
            .ingest_document(
                "bomb",
                "bomb.pdf",
                DocumentFormat::Pdf,
                &compressed,
                RIGHTS,
                ExtractionLimits {
                    max_output_bytes: 1_024,
                    ..ExtractionLimits::default()
                }
            )
            .unwrap_err(),
        EvidenceError::ExtractionLimit
    );
    let pages = pdf(&[
        (&"C".repeat(360), Variant::Flate),
        (&"D".repeat(360), Variant::Flate),
    ]);
    assert_eq!(
        ingest(
            &pages,
            ExtractionLimits {
                max_output_bytes: 600,
                ..ExtractionLimits::default()
            }
        )
        .unwrap_err(),
        EvidenceError::ExtractionLimit
    );
    assert_eq!(evidence.document_count(), 0);
}

#[test]
fn invalid_compressed_checksums_truncations_and_trailing_deflate_are_rejected() {
    for variant in [Variant::BadChecksum, Variant::Truncated, Variant::Trailing] {
        let bytes = pdf(&[("Private notes remain untouched", variant)]);
        let mut evidence = index();
        assert_eq!(
            evidence
                .ingest_document(
                    "bad",
                    "bad.pdf",
                    DocumentFormat::Pdf,
                    &bytes,
                    RIGHTS,
                    ExtractionLimits::default(),
                )
                .unwrap_err(),
            EvidenceError::InvalidDocument
        );
        assert_eq!(evidence.document_count(), 0);
    }
}

#[test]
fn unsupported_filters_parameters_and_chain_are_fail_closed_and_atomic() {
    for variant in [
        Variant::Predictor,
        Variant::Lzw,
        Variant::NestedFilters,
        Variant::BadFilter,
    ] {
        let bytes = pdf(&[("No automatic dangerous transformation", variant)]);
        let mut evidence = index();
        assert_eq!(
            evidence
                .ingest_document(
                    "bad",
                    "bad.pdf",
                    DocumentFormat::Pdf,
                    &bytes,
                    RIGHTS,
                    ExtractionLimits::default(),
                )
                .unwrap_err(),
            EvidenceError::UnsupportedDocument
        );
        assert_eq!(evidence.document_count(), 0);
    }
}

#[test]
fn encrypted_object_xref_and_escaped_names_cannot_reach_implicit_lopdf_decompression() {
    let valid = pdf(&[("Single correctly zipped page", Variant::Flate)]);
    for forbidden in [
        b" /Type /ObjStm".as_slice(),
        b" /Type /XRef".as_slice(),
        b" /XRefStm 3 0 R".as_slice(),
        b" /Encrypt 6 0 R".as_slice(),
        b" /F#69lter /FlateDecode".as_slice(),
        b" /Filter /LZWDecode".as_slice(),
    ] {
        let mut bytes = valid.clone();
        bytes.extend_from_slice(forbidden);
        assert_eq!(
            ingest(&bytes, ExtractionLimits::default()).unwrap_err(),
            EvidenceError::UnsupportedDocument,
            "forbidden token must be refused before lopdf parser"
        );
    }
}

#[test]
fn page_limit_and_binary_input_limit_apply_before_decompression() {
    let bytes = pdf(&[
        ("one", Variant::Flate),
        ("two", Variant::Flate),
        ("three", Variant::Flate),
    ]);
    assert_eq!(
        ingest(
            &bytes,
            ExtractionLimits {
                max_pages: 2,
                ..ExtractionLimits::default()
            }
        )
        .unwrap_err(),
        EvidenceError::ExtractionLimit
    );
    assert_eq!(
        ingest(
            &bytes,
            ExtractionLimits {
                max_input_bytes: 100,
                ..ExtractionLimits::default()
            }
        )
        .unwrap_err(),
        EvidenceError::ExtractionLimit
    );
}
