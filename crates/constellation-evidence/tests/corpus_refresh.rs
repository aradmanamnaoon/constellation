use constellation_evidence::{
    CorpusRefresh, EvidenceError, EvidenceIndex, IndexLimits, SourceFormat, SourceInput,
};

fn source<'a>(id: &'a str, path: &'a str, text: &'a str) -> SourceInput<'a> {
    SourceInput {
        source_id: id,
        relative_path: path,
        format: SourceFormat::PlainText,
        text,
    }
}

#[test]
fn replaced_and_removed_sources_disappear_from_all_search_results() {
    let mut index = EvidenceIndex::new(IndexLimits::default()).unwrap();
    index
        .replace_corpus([
            source("a", "notes/a.txt", "alpha orbital signal"),
            source("b", "notes/b.txt", "beta stable signal"),
        ])
        .unwrap();
    let before = index.search("orbital", 5).unwrap();
    assert_eq!(before.len(), 1);
    let previous_hash = before[0].source_sha256.to_string();

    let change = index
        .replace_corpus([
            source("a", "notes/a.txt", "alpha hippocampal signal"),
            source("c", "notes/c.txt", "gamma new signal"),
        ])
        .unwrap();
    assert_eq!(
        change,
        CorpusRefresh {
            added: 1,
            updated: 1,
            removed: 1,
            unchanged: 0,
        }
    );
    assert_eq!(index.document_count(), 2);
    assert!(index.search("orbital", 5).unwrap().is_empty());
    assert!(index.search("beta", 5).unwrap().is_empty());
    let hit = index.search("hippocampal", 5).unwrap();
    assert_eq!(hit.len(), 1);
    assert_ne!(hit[0].source_sha256, previous_hash);
    assert_eq!(hit[0].quote, "alpha hippocampal signal");
    assert_eq!(index.search("new", 5).unwrap()[0].source_id, "c");
}

#[test]
fn failed_refresh_leaves_previous_corpus_and_scores_untouched() {
    let mut index = EvidenceIndex::new(IndexLimits::default()).unwrap();
    index
        .ingest(source(
            "old",
            "papers/study.txt",
            "cortical stable observation",
        ))
        .unwrap();
    let hits_before = index.search("cortical", 5).unwrap();
    let original_score = hits_before[0].bm25_score;
    let original_hash = hits_before[0].source_sha256.to_owned();

    let result = index.replace_corpus([
        source("new", "papers/new.txt", "new only content"),
        source("bad", "../outside.txt", "should fail"),
    ]);
    assert_eq!(result.unwrap_err(), EvidenceError::InvalidSourcePath);
    assert_eq!(index.document_count(), 1);
    let hits = index.search("cortical", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].source_sha256, original_hash);
    assert_eq!(hits[0].bm25_score, original_score);
    assert!(index.search("new", 5).unwrap().is_empty());
}

#[test]
fn duplicate_path_and_oversized_corpus_fail_atomically_and_idempotency_reports_unchanged() {
    let limits = IndexLimits {
        max_total_bytes: 100,
        max_document_bytes: 100,
        max_chunk_bytes: 64,
        ..IndexLimits::default()
    };
    let mut index = EvidenceIndex::new(limits).unwrap();
    let unchanged = source("same", "notes/original.txt", "baseline cortex");
    index.replace_corpus([unchanged]).unwrap();
    assert_eq!(
        index.replace_corpus([unchanged]).unwrap(),
        CorpusRefresh {
            unchanged: 1,
            ..CorpusRefresh::default()
        }
    );

    assert_eq!(
        index
            .replace_corpus([
                unchanged,
                source("collision", "NOTES/ORIGINAL.TXT", "same path"),
            ])
            .unwrap_err(),
        EvidenceError::ConflictingPath
    );
    let too_long = "x ".repeat(48);
    assert_eq!(
        index
            .replace_corpus([unchanged, source("oversized", "big.txt", &too_long)])
            .unwrap_err(),
        EvidenceError::TotalByteLimit
    );
    assert_eq!(index.document_count(), 1);
    assert_eq!(index.search("cortex", 3).unwrap().len(), 1);
}
