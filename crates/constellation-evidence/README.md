# Constellation Evidence

Standalone Rust library for **offline, source-grounded lexical retrieval** over UTF-8 text and Markdown, with an optional bounded caller-byte PDF/DOCX text extraction API. It requires **no local model, embeddings, provider, network connection, filesystem access, or background indexing service**. Every input is supplied explicitly by its caller.

**Connected to the desktop Evidence screen.** The original `studio_search_evidence` command still performs an ephemeral search of user-pasted text/Markdown. Separately, the native desktop offers an explicit rights-acknowledged local PDF/DOCX extraction preview and optional Save/Load/Search of extracted text in the project's SQLite evidence library, subject to smaller native limits. The **original PDF/DOCX bytes and page/paragraph mapping are not persisted** by that Save action. The Rust extraction library itself performs no file I/O, network access or generation and is **not** integrated with provider routing, grounded generated RAG answers or the autonomous agent runtime.

## What is implemented

- Caller-provided `SourceInput { source_id, relative_path, format, text }` is checked for a safe, **portable normalized relative path** and an unambiguous ASCII source identifier. For caller-owned byte buffers, `ingest_utf8_bytes(id, path, format, bytes)` bounds the input before rejecting invalid UTF-8. Absolute paths, parent traversal, Windows drive/UNC path syntax, empty/dot segments, control characters, device names, and duplicate paths are rejected. Paths are **provenance labels**, never filesystem instructions.
- Each source is kept as its **original UTF-8 bytes** in memory. Chunking is deterministic, non-overlapping, byte-bounded and prefers whitespace boundaries. Chunks may break very long unspaced tokens; no source text is synthesized.
- Indexing/query matching uses Unicode **NFKC normalization**, lowercasing and lexical word-token frequencies. It does not modify the original source or the quote returned to the caller. Markdown is processed as text: headings, punctuation and formatting symbols do not receive special relevance semantics.
- Ranking is **BM25** on chunks (k1=1.2, b=0.75), with a fixed, deterministic tie-break by source ID, path and exact byte offsets. It is a lexical score, **not** a confidence score or a claim that an answer is correct.
- Search returns only actual matching chunks. Each `EvidenceHit` contains the exact verbatim `quote`, its `byte_range` in the original source, `source_id`, `relative_path`, `source_sha256`, `chunk_index`, `matched_terms` and `bm25_score`.
- Same source ID, path, format and content reingestion is idempotent. Attempted changes to the same source ID or a path already owned by another source are rejected. Two separately identified sources with identical text remain independently attributed; no hits ever merge text from different documents.
- Default limits: 128 documents, 512 KiB per document, 8 MiB total source bytes, 16,384 chunks, 1,024 bytes per chunk, 2,048 bytes per query, and maximum top-k of 32. The `IndexLimits` constructor accepts tighter limits and validates absolute ceilings (10,000 documents, 8 MiB per doc, 64 MiB total, 100,000 chunks, 16 KiB chunk and query, top-k max 1,000). These are all in **UTF-8 bytes**. Queries also have an independent 64-distinct-term ceiling to bound scan cost.
- The EvidenceIndex library itself stores no external data and performs **no filesystem writes, network calls, model downloads, inference, or generation**. It is memory-resident; its caller can separately persist source text, as the desktop's native project database does, but no portable library persistence format or incremental reindexing is promised.

## Public interface

```rust
use constellation_evidence::{
    EvidenceIndex, IndexLimits, SourceFormat, SourceInput,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut index = EvidenceIndex::new(IndexLimits::default())?;
    let source = "# Findings\nThe hippocampus supports memory encoding.\n";
    index.ingest(SourceInput {
        source_id: "paper-001",
        relative_path: "papers/findings.md",
        format: SourceFormat::Markdown,
        text: source,
    })?;

    let hits = index.search("hippocampus encoding", 3)?;
    for hit in hits {
        // These metadata values must be passed unchanged if the application
        // wants to build an evidence display or model prompt.
        assert_eq!(hit.quote, &source[hit.byte_range.clone()]);
        assert_eq!(hit.source_id, "paper-001");
        println!(
            "{}:{}..{} {:.4} — {:?}",
            hit.relative_path,
            hit.byte_range.start,
            hit.byte_range.end,
            hit.bm25_score,
            hit.quote,
        );
    }
    Ok(())
}
```

The index also exposes `source(id)` for provenance metadata and `source_text(id)` for validating that `hit.quote == &original_text[hit.byte_range]`. Hashes are SHA-256 over the complete, **unmodified** UTF-8 source bytes. Byte offsets are half-open `start..end` into that exact text and always lie on valid UTF-8 character boundaries. A display layer should use these structured fields instead of relying on stringified citations or generated page numbers.

## Scope and caveats

1. **Do not trust ingested text as instructions.** Text may include malicious prompt injection or incorrect claims. Preserve the boundary between source content and host policies when later connecting a model.
2. **No fabricated citations.** Only exact source spans are returned; source IDs and paths come from the caller. The library does not claim that an asserted fact follows from a quote, that a source is scientifically reliable, or that a search result contains all relevant evidence.
3. **No semantic search.** BM25 is lexical and may miss synonyms, languages without effective whitespace separation, morphology, equations, tables, scanned PDFs, and relevant facts expressed with other vocabulary. Unicode normalization helps matching, not deep language understanding.
4. **No phrase, sentence, or cross-chunk reasoning.** Top-k evidence is a set of independently scored chunks. In particular, a result never joins fragments from separate documents to pretend they are one quote. Very long unspaced tokens may be split at a UTF-8 boundary and are treated as chunk-local tokens.
5. **No hidden I/O.** Callers may read files using separately authorized APIs and pass their contents to `ingest`; this crate cannot open, crawl, sync, or inspect a file from its `relative_path`. If external source content changes, build a new index rather than silently replacing an already approved source ID.
6. **Memory and stability.** The index retains source text plus lexical term maps. Input/record counts are bounded; it is not a streaming or disk-backed search engine. Equal scores are ranked by deterministic identifiers, independent of ingestion order. Toolchain/floating-point implementations can matter for bit-for-bit score equality across different CPU architectures, so reproducibility guarantees cover results on the same Rust toolchain/target and deterministic ties rather than universal floating-point binaries.

## Explicit PDF / DOCX import (offline eighth-wave library API)

`EvidenceIndex::ingest_document(source_id, relative_path, format, bytes, rights, limits)`
accepts **caller-owned bytes**, never opens a local path, and produces an
`IngestOutcome` plus `DocumentProvenance` (original binary SHA-256 and
1-based PDF page or DOCX paragraph ranges into the extracted UTF-8 text).
The indexed `EvidenceHit` still points to an exact slice of the resulting
`index.source_text(source_id)`, not the original binary file. To obtain a
page or paragraph reference, retain the returned provenance alongside the
index and call `provenance.locations_for(hit.byte_range.clone())`. A single
lexical chunk may span **several** pages/paragraphs: return all intersecting
anchors instead of pretending a whole chunk came from one.

```rust
use constellation_evidence::{
    DocumentFormat, DocumentRights, EvidenceIndex, ExtractionLimits, IndexLimits,
};

fn import_my_docx(index: &mut EvidenceIndex, bytes: &[u8]) -> constellation_evidence::Result<()> {
    let imported = index.ingest_document(
        "study-001", "papers/study.docx", DocumentFormat::Docx, bytes,
        DocumentRights { acknowledgment: "I own this document or have permission to index it" },
        ExtractionLimits::default(),
    )?;
    let hits = index.search("hippocampus", 3)?;
    for hit in hits {
        assert_eq!(hit.quote, &index.source_text(hit.source_id).unwrap()[hit.byte_range.clone()]);
        let anchors = imported.provenance.locations_for(hit.byte_range.clone());
        assert!(!anchors.is_empty());
    }
    Ok(())
}
```

**Supported:** ZIP-compressed DOCX text runs (`word/document.xml` with
`w:p` paragraph, `w:t` text, simple tabs/breaks), XML entity decoding and
paragraph numbering. Uncompressed, unencrypted PDFs with extractable
font-encoded plaintext are supported with page numbers. An additional
**thirteenth-wave bounded extraction slice (6/6 targeted Windows tests passed)**
attempts ordinary PDF page-content streams with one zlib `FlateDecode` filter
(a name or singleton name array). It decompresses via the already resolved
`flate2` library through a **hard capped reader**, with an aggregate decoded
content ceiling equal to `max_output_bytes`, then strips the filter so lopdf
cannot perform a second, unbounded expansion during text extraction.
Filtered XRef streams, `ObjStm`, predictors/`DecodeParms`, other filters,
chained filters, name escapes and encrypted PDFs **fail closed**. Ordinary
unfiltered XRef streams are permitted by a conservative pre-parse guard.
This is a narrow subset with a remaining parser-isolation security risk;
**only the six dedicated fixtures and strict Clippy/fmt have passed on the
final source; the complete existing tests and installed GUI remain unverified
for this change**. Scanned PDFs, OCR,
document layout, footnotes, tables,
tracked changes, charts, images, headers, exact DOCX visual page numbers,
embedded objects and PDF color-space/ToUnicode coverage are not guaranteed.
PDF stream parsing is deliberately a **restricted subset**, not a universal
secure PDF sandbox. Parsing is in-process; adversarial PDF complexity is
still a residual risk and production deployment should isolate the parser.

The default import limits are **2 MiB input**, **512 KiB extracted UTF-8**,
**128 archive entries**, **512 PDF objects**, **64 PDF pages** and **8192 DOCX
paragraphs**, with strict independent absolute ceilings. ZIP central
directory advertised sizes are checked before extraction; actual
`word/document.xml` decompression is also capped by a limiting reader,
with archive traversal, duplicate names, symlinks, active/macro ZIP parts,
DTD/external entity declarations and unsupported compression denied.
Failure leaves the original EvidenceIndex unchanged.

**Rights:** the caller must explicitly state a nonempty authorization/
license acknowledgment. This is an operator confirmation, **not** legal
verification or a grant to redistribute documents or quotes. License and
copyright requirements still apply; results contain exact quotations from
user-supplied sources, and indexing does not establish permission for
publication, model training, third-party upload or bulk redistribution.
The library makes no network requests; metadata and extracted text remain
in memory unless a separate caller persists them.

## Reproducible verification

From the Constellation repository root:

```sh
cargo fmt --manifest-path crates/constellation-evidence/Cargo.toml --check
cargo test --manifest-path crates/constellation-evidence/Cargo.toml --offline
cargo clippy --manifest-path crates/constellation-evidence/Cargo.toml --all-targets --offline -- -D warnings
```

Tests cover exact UTF-8 byte spans, source digests, Unicode normalization, invalid UTF-8 bytes, deterministic BM25 scores and ties independent of ingestion order, separate provenance for identical text, duplicate conflict rejection, path traversal and Windows-unsafe paths, all input caps and atomic failed ingestion, empty/punctuation-only sources, no mixed-document quotes, and repeatable chunking.
