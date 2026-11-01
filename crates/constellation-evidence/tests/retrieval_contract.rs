use constellation_evidence::{
    EvidenceError, EvidenceIndex, IndexLimits, IngestOutcome, SourceFormat, SourceInput,
};
use sha2::{Digest, Sha256};

fn source<'a>(id: &'a str, path: &'a str, format: SourceFormat, text: &'a str) -> SourceInput<'a> {
    SourceInput {
        source_id: id,
        relative_path: path,
        format,
        text,
    }
}

fn index() -> EvidenceIndex {
    EvidenceIndex::new(IndexLimits::default()).unwrap()
}

#[test]
fn exact_spans_and_hashes_are_grounded_in_original_sources() {
    let text = "# Alpha observation\n\nPatient-free phantom data: \u{1f9e0} cortex and a café.\n";
    let mut index = index();
    let outcome = index
        .ingest(source(
            "study-01",
            "papers/evidence.md",
            SourceFormat::Markdown,
            text,
        ))
        .unwrap();
    assert!(matches!(outcome, IngestOutcome::Added { chunks: 1 }));
    let info = index.source("study-01").unwrap();
    assert_eq!(info.relative_path, "papers/evidence.md");
    assert_eq!(info.source_bytes, text.len());
    assert_eq!(info.format, SourceFormat::Markdown);
    assert_eq!(
        info.sha256,
        format!("{:x}", Sha256::digest(text.as_bytes()))
    );
    let hits = index.search("phantom cortex", 3).unwrap();
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.source_id, "study-01");
    assert_eq!(hit.relative_path, "papers/evidence.md");
    assert_eq!(hit.source_sha256, info.sha256);
    assert_eq!(hit.quote, &text[hit.byte_range.clone()]);
    assert!(text.is_char_boundary(hit.byte_range.start));
    assert!(text.is_char_boundary(hit.byte_range.end));
    assert!(hit.quote.contains("cortex"));
    assert!(hit.bm25_score.is_finite());
    assert!(hit.bm25_score > 0.0);
    assert_eq!(hit.matched_terms, ["cortex", "phantom"]);
}

#[test]
fn unicode_search_is_nfkc_normalized_but_quotes_remain_verbatim() {
    let mut index = index();
    let original = "Cafe\u{0301} — ﻋﻠﻢ — 🧠 — فارسی";
    index
        .ingest(source(
            "unicode",
            "notes/داده.md",
            SourceFormat::Markdown,
            original,
        ))
        .unwrap();
    for query in ["CAFÉ", "ﻋﻠﻢ", "فارسی"] {
        let hits = index.search(query, 3).unwrap();
        assert_eq!(hits.len(), 1, "missing normalized query {query}");
        let span = hits[0].byte_range.clone();
        assert_eq!(hits[0].quote, &original[span]);
        assert!(hits[0].quote.contains("Cafe\u{0301}"));
    }
    assert!(index.search("unknown-lexeme-987", 3).unwrap().is_empty());
}

#[test]
fn deterministic_bm25_ranking_and_ties_ignore_ingestion_order() {
    let docs = [
        ("zeta", "zeta.md", "neuron neuron memory memory cortex"),
        ("alpha", "alpha.md", "neuron memory cortex"),
        ("beta", "beta.md", "brain neuron cortex"),
    ];
    let mut first = index();
    for (id, path, body) in docs {
        first
            .ingest(source(id, path, SourceFormat::PlainText, body))
            .unwrap();
    }
    let mut reverse = index();
    for (id, path, body) in docs.into_iter().rev() {
        reverse
            .ingest(source(id, path, SourceFormat::PlainText, body))
            .unwrap();
    }
    let ranked_first = first.search("memory neuron", 3).unwrap();
    let ranked_reverse = reverse.search("memory neuron", 3).unwrap();
    assert_eq!(ranked_first, ranked_reverse);
    assert_eq!(ranked_first.len(), 3);
    assert!(ranked_first[0].bm25_score >= ranked_first[1].bm25_score);
    // Repeating a query token does not change the score.
    assert_eq!(
        first.search("memory neuron neuron", 3).unwrap(),
        ranked_first
    );

    let mut ties = index();
    ties.ingest(source("b", "b.txt", SourceFormat::PlainText, "same text"))
        .unwrap();
    ties.ingest(source("a", "a.txt", SourceFormat::PlainText, "same text"))
        .unwrap();
    let ranked = ties.search("same", 2).unwrap();
    assert_eq!((ranked[0].source_id, ranked[1].source_id), ("a", "b"));
    assert_eq!(ranked[0].bm25_score, ranked[1].bm25_score);
}

#[test]
fn evidence_never_combines_text_from_two_documents() {
    let mut index = index();
    let one = "The unique kiwi observations.";
    let two = "The unique zebra measurements.";
    index
        .ingest(source("kiwi", "kiwi.txt", SourceFormat::PlainText, one))
        .unwrap();
    index
        .ingest(source("zebra", "zebra.txt", SourceFormat::PlainText, two))
        .unwrap();
    let hits = index.search("kiwi zebra", 8).unwrap();
    assert_eq!(hits.len(), 2);
    for hit in hits {
        let original = index.source_text(hit.source_id).unwrap();
        assert_eq!(hit.quote, &original[hit.byte_range.clone()]);
        assert!(!(hit.quote.contains("kiwi") && hit.quote.contains("zebra")));
        assert_eq!(hit.matched_terms.len(), 1);
    }
}

#[test]
fn independent_sources_with_identical_content_keep_distinct_provenance() {
    let mut index = index();
    let text = "identical scientific observation";
    index
        .ingest(source("one", "a.txt", SourceFormat::PlainText, text))
        .unwrap();
    index
        .ingest(source("two", "b.txt", SourceFormat::PlainText, text))
        .unwrap();
    assert_eq!(index.document_count(), 2);
    let hits = index.search("scientific", 5).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].source_id, "one");
    assert_eq!(hits[1].source_id, "two");
    assert_eq!(hits[0].source_sha256, hits[1].source_sha256);
    assert_ne!(hits[0].relative_path, hits[1].relative_path);
}

#[test]
fn repeated_ingestion_is_idempotent_conflicts_fail_closed() {
    let mut index = index();
    let original = source(
        "Doc",
        "studies/cortex.md",
        SourceFormat::Markdown,
        "cortex observation",
    );
    assert_eq!(
        index.ingest(original).unwrap(),
        IngestOutcome::Added { chunks: 1 }
    );
    assert_eq!(
        index.ingest(original).unwrap(),
        IngestOutcome::AlreadyPresent
    );
    assert_eq!(index.document_count(), 1);
    assert_eq!(index.total_source_bytes(), "cortex observation".len());
    assert_eq!(index.chunk_count(), 1);
    assert_eq!(
        index
            .ingest(source(
                "doc",
                "studies/cortex.md",
                SourceFormat::Markdown,
                "different"
            ))
            .unwrap_err(),
        EvidenceError::ConflictingSourceId
    );
    assert_eq!(
        index
            .ingest(source(
                "doc",
                "studies/other.md",
                SourceFormat::Markdown,
                "cortex observation"
            ))
            .unwrap_err(),
        EvidenceError::ConflictingSourceId
    );
    assert_eq!(
        index
            .ingest(source(
                "other",
                "studies/CORTEX.md",
                SourceFormat::PlainText,
                "another"
            ))
            .unwrap_err(),
        EvidenceError::ConflictingPath
    );
    assert_eq!(index.document_count(), 1);
    assert_eq!(index.chunk_count(), 1);
}

#[test]
fn reject_traversal_absolute_windows_devices_unsafe_segments_and_invalid_ids() {
    let mut index = index();
    let rejected = [
        "../secret.txt",
        "folder/../secret.txt",
        "./readme.md",
        "folder/./readme.md",
        "/root/secret.txt",
        "C:/Users/secret.txt",
        "C:\\Users\\secret.txt",
        "folder\\secret.txt",
        "folder//file",
        "file/",
        "CON.txt",
        "folder/nul.txt",
        "LPT1",
        "folder/COM4.log",
        "file.",
        "folder/abc ",
        "folder\0doc.md",
    ];
    for (i, path) in rejected.iter().enumerate() {
        let id = format!("doc{i}");
        assert_eq!(
            index
                .ingest(source(&id, path, SourceFormat::PlainText, "safe"))
                .unwrap_err(),
            EvidenceError::InvalidSourcePath,
            "accepted unsafe path {path:?}"
        );
    }
    for id in [
        "",
        "../bad",
        "doc/id",
        "abc..xyz",
        "-starts-dash",
        "has space",
        "💀",
    ] {
        assert_eq!(
            index
                .ingest(source(id, "safe.md", SourceFormat::Markdown, "test"))
                .unwrap_err(),
            EvidenceError::InvalidSourceId,
            "accepted unsafe id {id:?}"
        );
    }
    assert_eq!(index.document_count(), 0);
}

#[test]
fn reject_empty_whitespace_and_nonsearchable_punctuation_only_sources() {
    let mut index = index();
    for (i, text) in ["", "   \n\t", "--- ### !!!"].iter().enumerate() {
        let id = format!("empty{i}");
        assert_eq!(
            index
                .ingest(source(
                    &id,
                    &format!("{id}.md"),
                    SourceFormat::Markdown,
                    text
                ))
                .unwrap_err(),
            EvidenceError::EmptySource
        );
    }
    assert_eq!(index.document_count(), 0);
}

fn small_limits() -> IndexLimits {
    IndexLimits {
        max_documents: 2,
        max_document_bytes: 48,
        max_total_bytes: 64,
        max_chunks: 3,
        max_chunk_bytes: 16,
        max_query_bytes: 24,
        max_top_k: 2,
    }
}

#[test]
fn bounded_documents_bytes_chunks_and_queries_are_enforced_atomically() {
    let mut index = EvidenceIndex::new(small_limits()).unwrap();
    assert_eq!(
        index
            .ingest(source(
                "long",
                "long.txt",
                SourceFormat::PlainText,
                &"x".repeat(49)
            ))
            .unwrap_err(),
        EvidenceError::DocumentByteLimit
    );
    // Single document would require more than three chunks; no partial commit.
    assert_eq!(
        index
            .ingest(source(
                "too-many",
                "chunks.txt",
                SourceFormat::PlainText,
                "alpha beta gamma delta epsilon zeta eta theta"
            ))
            .unwrap_err(),
        EvidenceError::ChunkLimit
    );
    assert_eq!(index.document_count(), 0);
    assert_eq!(index.chunk_count(), 0);
    index
        .ingest(source(
            "first",
            "first.txt",
            SourceFormat::PlainText,
            "alpha beta",
        ))
        .unwrap();
    index
        .ingest(source(
            "second",
            "second.txt",
            SourceFormat::PlainText,
            "gamma delta",
        ))
        .unwrap();
    assert_eq!(
        index
            .ingest(source(
                "third",
                "third.txt",
                SourceFormat::PlainText,
                "epsilon"
            ))
            .unwrap_err(),
        EvidenceError::DocumentLimit
    );
    assert_eq!(index.document_count(), 2);
    assert_eq!(
        index.search(&"long".repeat(7), 1).unwrap_err(),
        EvidenceError::InvalidQuery
    );
    assert_eq!(
        index.search("###", 1).unwrap_err(),
        EvidenceError::InvalidQuery
    );
    assert_eq!(
        index.search("alpha", 0).unwrap_err(),
        EvidenceError::InvalidTopK
    );
    assert_eq!(
        index.search("alpha", 3).unwrap_err(),
        EvidenceError::InvalidTopK
    );
}

#[test]
fn total_bytes_and_chunk_limits_are_independent() {
    let mut limits = small_limits();
    limits.max_documents = 3;
    limits.max_total_bytes = 20;
    limits.max_document_bytes = 20;
    let mut index = EvidenceIndex::new(limits).unwrap();
    index
        .ingest(source("a", "a", SourceFormat::PlainText, "eleven----"))
        .unwrap();
    assert_eq!(
        index
            .ingest(source("b", "b", SourceFormat::PlainText, "twelve eleven"))
            .unwrap_err(),
        EvidenceError::TotalByteLimit
    );
    assert_eq!(index.document_count(), 1);
}

#[test]
fn very_small_chunk_bytes_are_utf8_safe_and_exact() {
    let limits = IndexLimits {
        max_document_bytes: 32,
        max_total_bytes: 32,
        max_chunk_bytes: 4,
        ..small_limits()
    };
    let mut index = EvidenceIndex::new(limits).unwrap();
    let input = "漢字";
    assert_eq!(
        index
            .ingest(source("a", "a.txt", SourceFormat::PlainText, input))
            .unwrap(),
        IngestOutcome::Added { chunks: 2 }
    );
    let hits = index.search("漢", 2).unwrap();
    assert_eq!(hits[0].quote, "漢");
    assert_eq!(hits[0].byte_range, 0..3);
    assert_eq!(hits[0].quote, &input[hits[0].byte_range.clone()]);
    // Two additional chunks exceed the index-wide three-chunk budget.
    assert_eq!(
        index
            .ingest(source("b", "b.txt", SourceFormat::PlainText, "éééé"))
            .unwrap_err(),
        EvidenceError::ChunkLimit
    );
}

#[test]
fn document_limits_validate_configuration_without_allocating() {
    let invalid = IndexLimits {
        max_documents: 0,
        ..IndexLimits::default()
    };
    assert!(matches!(
        EvidenceIndex::new(invalid),
        Err(EvidenceError::InvalidLimits)
    ));
    let invalid = IndexLimits {
        max_total_bytes: 1,
        ..IndexLimits::default()
    };
    assert!(matches!(
        EvidenceIndex::new(invalid),
        Err(EvidenceError::InvalidLimits)
    ));
    let invalid = IndexLimits {
        max_chunk_bytes: 3,
        ..IndexLimits::default()
    };
    assert!(matches!(
        EvidenceIndex::new(invalid),
        Err(EvidenceError::InvalidLimits)
    ));
}

#[test]
fn caller_owned_raw_bytes_require_valid_utf8_and_respect_caps() {
    let mut limits = small_limits();
    limits.max_document_bytes = 32;
    let mut index = EvidenceIndex::new(limits).unwrap();
    assert_eq!(
        index
            .ingest_utf8_bytes("bad", "bad.md", SourceFormat::Markdown, &[0xff, 0xfe])
            .unwrap_err(),
        EvidenceError::InvalidUtf8
    );
    let too_large = vec![b'a'; 33];
    assert_eq!(
        index
            .ingest_utf8_bytes("large", "large.md", SourceFormat::Markdown, &too_large)
            .unwrap_err(),
        EvidenceError::DocumentByteLimit
    );
    assert_eq!(index.document_count(), 0);
    assert_eq!(
        index
            .ingest_utf8_bytes("ok", "ok.md", SourceFormat::Markdown, "مدار".as_bytes())
            .unwrap(),
        IngestOutcome::Added { chunks: 1 }
    );
    assert_eq!(index.search("مدار", 1).unwrap()[0].quote, "مدار");
}

#[test]
fn query_unique_token_budget_prevents_large_cross_product_scans() {
    let mut index = index();
    index
        .ingest(source(
            "one",
            "one.md",
            SourceFormat::Markdown,
            "unique searchable",
        ))
        .unwrap();
    let sixty_five = (0..65)
        .map(|i| format!("term{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        index.search(&sixty_five, 1).unwrap_err(),
        EvidenceError::InvalidQuery
    );
    // Repeating the same term has no effect on BM25 and consumes one term.
    assert_eq!(
        index.search("unique unique unique", 1).unwrap(),
        index.search("unique", 1).unwrap()
    );
}

#[test]
fn canonically_equivalent_paths_cannot_claim_two_source_ids() {
    let mut index = index();
    index
        .ingest(source(
            "one",
            "papers/café.md",
            SourceFormat::Markdown,
            "cortex",
        ))
        .unwrap();
    assert_eq!(
        index
            .ingest(source(
                "two",
                "papers/cafe\u{0301}.md",
                SourceFormat::Markdown,
                "neuron"
            ))
            .unwrap_err(),
        EvidenceError::ConflictingPath
    );
    assert_eq!(index.document_count(), 1);
}

#[test]
fn chunking_is_repeatable_and_preserves_utf8_byte_ranges_across_boundaries() {
    let limits = IndexLimits {
        max_chunk_bytes: 20,
        ..IndexLimits::default()
    };
    let input = "Cortex makes sense.\n\nNeuroplasticité demeure.\n漢字漢字漢字";
    let build = || {
        let mut index = EvidenceIndex::new(limits).unwrap();
        index
            .ingest(source("one", "x.md", SourceFormat::Markdown, input))
            .unwrap();
        index
    };
    let one = build();
    let two = build();
    let first = one.search("cortex neuroplasticité 漢字", 12).unwrap();
    let second = two.search("cortex neuroplasticité 漢字", 12).unwrap();
    assert_eq!(first, second);
    for hit in first {
        assert_eq!(hit.quote, &input[hit.byte_range]);
        assert!(hit.quote.len() <= limits.max_chunk_bytes);
    }
}
