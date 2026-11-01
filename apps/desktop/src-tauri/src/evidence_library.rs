//! Explicitly saved local evidence, in the same project SQLite file as the
//! existing encrypted archive. Pasted-only search stays memory-only.
//! No filesystem ingestion, model request, or network access is reachable here.
use std::{path::Path, time::Duration};

use constellation_core::{EVIDENCE_LIBRARY_DDL, verify_initialized_schema};
use constellation_evidence::{EvidenceIndex, IndexLimits, SourceFormat, SourceInput};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior, params};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_TITLE_BYTES: usize = 120;
const MAX_DOCUMENT_BYTES: usize = 128 * 1024;
const MAX_DOCUMENTS: usize = 128;
const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SavedSource {
    pub source_id: String,
    pub title: String,
    pub source_sha256: String,
    pub bytes: usize,
    pub format: String,
    pub relative_path: String,
    pub text: String,
}

fn unavailable() -> String {
    "Saved evidence is unavailable or its project database is not trusted".into()
}

fn limits() -> IndexLimits {
    IndexLimits {
        max_documents: MAX_DOCUMENTS,
        max_document_bytes: MAX_DOCUMENT_BYTES,
        max_total_bytes: MAX_TOTAL_BYTES,
        max_chunks: 16_384,
        max_chunk_bytes: 1024,
        max_query_bytes: 2048,
        max_top_k: 10,
    }
}

fn open(path: &Path, write: bool) -> Result<Connection, String> {
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    // No create flag: only the audited Core initializer provisions a project.
    let conn = Connection::open_with_flags(path, flags).map_err(|_| unavailable())?;
    conn.busy_timeout(Duration::from_secs(4))
        .map_err(|_| unavailable())?;
    Ok(conn)
}

fn read_sources(tx: &Transaction<'_>) -> Result<Vec<SavedSource>, String> {
    verify_initialized_schema(tx).map_err(|_| unavailable())?;
    let exists: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='evidence_sources'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| unavailable())?;
    if exists == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = tx
        .prepare(
            "SELECT source_id,title,source_sha256,source_bytes,source_format,source_text
         FROM evidence_sources ORDER BY source_id",
        )
        .map_err(|_| unavailable())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|_| unavailable())?;
    let mut sources = Vec::new();
    for row in rows {
        let (id, title, sha, bytes, format, content) = row.map_err(|_| unavailable())?;
        let byte_count = usize::try_from(bytes).map_err(|_| unavailable())?;
        if uuid::Uuid::parse_str(&id).is_err()
            || id.len() != 36
            || title.is_empty()
            || title.len() > MAX_TITLE_BYTES
            || title.chars().any(char::is_control)
            || !matches!(format.as_str(), "markdown" | "plain_text")
            || content.is_empty()
            || content.len() != byte_count
            || byte_count > MAX_DOCUMENT_BYTES
            || sha != format!("{:x}", Sha256::digest(content.as_bytes()))
        {
            return Err(unavailable());
        }
        let extension = if format == "markdown" { "md" } else { "txt" };
        sources.push(SavedSource {
            source_id: id.clone(),
            title,
            source_sha256: sha,
            bytes: byte_count,
            format,
            relative_path: format!("evidence/{id}.{extension}"),
            text: content,
        });
        if sources.len() > MAX_DOCUMENTS {
            return Err(unavailable());
        }
    }
    Ok(sources)
}

fn index_sources(sources: &[SavedSource]) -> Result<EvidenceIndex, String> {
    let mut index = EvidenceIndex::new(limits()).map_err(|_| unavailable())?;
    for source in sources {
        let format = if source.format == "markdown" {
            SourceFormat::Markdown
        } else {
            SourceFormat::PlainText
        };
        index
            .ingest(SourceInput {
                source_id: &source.source_id,
                relative_path: &source.relative_path,
                format,
                text: &source.text,
            })
            .map_err(|_| unavailable())?;
    }
    Ok(index)
}

pub(crate) fn save(path: &Path, title: &str, text: &str) -> Result<SavedSource, String> {
    let title = title.trim();
    if title.is_empty()
        || title.len() > MAX_TITLE_BYTES
        || title.chars().any(char::is_control)
        || text.is_empty()
        || text.len() > MAX_DOCUMENT_BYTES
    {
        return Err("Give the source a short safe title and at most 128 KiB of text".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let source = SavedSource {
        source_id: id.clone(),
        title: title.to_owned(),
        source_sha256: format!("{:x}", Sha256::digest(text.as_bytes())),
        bytes: text.len(),
        format: "markdown".into(),
        relative_path: format!("evidence/{id}.md"),
        text: text.to_owned(),
    };
    let mut db = open(path, true)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| unavailable())?;
    // Stage the *entire* potential corpus for quota/provenance validation
    // before any SQLite writes. Failed saves cannot corrupt existing quotes.
    let mut sources = read_sources(&tx)?;
    sources.push(source.clone());
    index_sources(&sources)?;
    tx.execute_batch(EVIDENCE_LIBRARY_DDL)
        .map_err(|_| unavailable())?;
    verify_initialized_schema(&tx).map_err(|_| unavailable())?;
    tx.execute(
        "INSERT INTO evidence_sources
         (source_id,title,source_format,source_text,source_sha256,source_bytes)
         VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            source.source_id,
            source.title,
            source.format,
            source.text,
            source.source_sha256,
            source.bytes as i64
        ],
    )
    .map_err(|_| unavailable())?;
    tx.commit().map_err(|_| unavailable())?;
    Ok(source)
}

pub(crate) fn list(path: &Path) -> Result<Vec<Value>, String> {
    let mut db = open(path, false)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| unavailable())?;
    let sources = read_sources(&tx)?;
    index_sources(&sources)?; // also rejects a malformed persisted source
    tx.commit().map_err(|_| unavailable())?;
    Ok(sources
        .into_iter()
        .map(|s| {
            json!({
                "source_id": s.source_id, "title": s.title,
                "relative_path": s.relative_path, "source_sha256": s.source_sha256,
                "bytes": s.bytes, "format": s.format
            })
        })
        .collect())
}

pub(crate) fn load(path: &Path, source_id: &str) -> Result<SavedSource, String> {
    if uuid::Uuid::parse_str(source_id).is_err() {
        return Err(unavailable());
    }
    let mut db = open(path, false)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| unavailable())?;
    let sources = read_sources(&tx)?;
    index_sources(&sources)?;
    let source = sources
        .into_iter()
        .find(|item| item.source_id == source_id)
        .ok_or_else(unavailable)?;
    tx.commit().map_err(|_| unavailable())?;
    Ok(source)
}

/// Delete only the specific source the user reviewed. The client must supply
/// both the random ID and its expected content hash; a stale confirmation,
/// rewritten row or second deletion cannot silently remove another source.
/// We validate the entire corpus before and after a single immediate SQLite
/// transaction, so malformed unrelated rows never authorize a partial edit.
pub(crate) fn delete(path: &Path, source_id: &str, expected_sha256: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(source_id).is_err()
        || source_id.len() != 36
        || expected_sha256.len() != 64
        || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(unavailable());
    }
    let mut db = open(path, true)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| unavailable())?;
    let mut sources = read_sources(&tx)?;
    index_sources(&sources)?;
    let index = sources
        .iter()
        .position(|source| source.source_id == source_id)
        .ok_or_else(unavailable)?;
    if !sources[index]
        .source_sha256
        .eq_ignore_ascii_case(expected_sha256)
    {
        return Err(unavailable());
    }
    sources.remove(index);
    index_sources(&sources)?;
    let changed = tx
        .execute(
            "DELETE FROM evidence_sources WHERE source_id=?1 AND source_sha256=?2",
            params![source_id, expected_sha256.to_ascii_lowercase()],
        )
        .map_err(|_| unavailable())?;
    if changed != 1 {
        return Err(unavailable());
    }
    tx.commit().map_err(|_| unavailable())
}

pub(crate) fn search(path: &Path, query: &str) -> Result<Value, String> {
    let mut db = open(path, false)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| unavailable())?;
    let sources = read_sources(&tx)?;
    let index = index_sources(&sources)?;
    let hits = index
        .search(query, 10)
        .map_err(|_| "Search needs 1–2048 bytes of valid words".to_string())?;
    let results = hits
        .into_iter()
        .map(|hit| {
            json!({
                "source_id": hit.source_id, "relative_path": hit.relative_path,
                "source_sha256": hit.source_sha256,
                "start_byte": hit.byte_range.start, "end_byte": hit.byte_range.end,
                "quote": hit.quote, "bm25_score": hit.bm25_score,
            })
        })
        .collect::<Vec<_>>();
    tx.commit().map_err(|_| unavailable())?;
    Ok(json!({"results": results, "stored": true, "model_called": false}))
}

fn literal_quote_fence(quote: &str) -> String {
    // A user source is untrusted Markdown/HTML. Pick a fence strictly longer
    // than any sequence of tildes inside it, so the exact quote can never
    // break out and inject headings, links or raw HTML into the export.
    let mut longest = 0usize;
    let mut current = 0usize;
    for byte in quote.bytes() {
        if byte == b'~' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    let delimiter = "~".repeat(longest.saturating_add(1).max(3));
    format!("{delimiter}text\n{quote}\n{delimiter}\n")
}

/// Human-triggered export of search hits. Exact source byte offsets and
/// hashes remain attached; no model-generated claims or network action.
/// The result is returned in memory for a separate explicit clipboard action.
pub(crate) fn export_markdown(path: &Path, query: &str) -> Result<String, String> {
    let mut db = open(path, false)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| unavailable())?;
    let sources = read_sources(&tx)?;
    let index = index_sources(&sources)?;
    let hits = index
        .search(query, 10)
        .map_err(|_| "Search needs 1–2048 bytes of valid words".to_string())?;
    let mut output = String::from(
        "# Constellation saved evidence excerpts\n\nVerbatim local quotations only. Relevance scores are lexical, not verification or conclusions. Retain and inspect original sources before citing or sharing.\n\n",
    );
    for (i, hit) in hits.iter().enumerate() {
        output.push_str(&format!(
            "## Excerpt {}\n\nSource ID: `{}`\nPath: `{}`\nSource SHA-256: `{}`\nUTF-8 byte range: [{}, {})\n\n",
            i + 1, hit.source_id, hit.relative_path, hit.source_sha256,
            hit.byte_range.start, hit.byte_range.end,
        ));
        output.push_str(&literal_quote_fence(hit.quote));
        output.push('\n');
    }
    if hits.is_empty() {
        output.push_str("No matching excerpts.\n");
    }
    tx.commit().map_err(|_| unavailable())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use constellation_core::StudioStore;

    #[test]
    fn explicit_saved_sources_survive_restart_and_retain_verbatim_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        let core = StudioStore::open(&path).unwrap();
        let room = core.create_room("Saved evidence project").unwrap();
        drop(core);
        let a = save(&path, "Paper A", "The hippocampus supports memory.").unwrap();
        let b = save(&path, "Paper B", "The cortex processes signals.").unwrap();
        assert_ne!(a.source_id, b.source_id);
        let results = search(&path, "hippocampus").unwrap();
        let hit = &results["results"][0];
        assert_eq!(hit["source_id"], a.source_id);
        assert_eq!(hit["quote"], "The hippocampus supports memory.");
        assert_eq!(list(&path).unwrap().len(), 2);
        let restored = load(&path, &a.source_id).unwrap();
        assert_eq!(restored.text, "The hippocampus supports memory.");
        let core = StudioStore::open(&path).unwrap();
        assert!(core.snapshot().unwrap().rooms.contains(&room));
    }

    #[test]
    fn invalid_or_tampered_evidence_never_displaces_existing_corpus() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        drop(StudioStore::open(&path).unwrap());
        let saved = save(&path, "Original", "Original verified statement.").unwrap();
        assert!(save(&path, "Bad", "   ").is_err()); // no searchable words
        assert!(save(&path, "x", &"X".repeat(MAX_DOCUMENT_BYTES + 1)).is_err());
        assert_eq!(list(&path).unwrap().len(), 1);
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE evidence_sources SET source_text='Forged text.' WHERE source_id=?1",
            [&saved.source_id],
        )
        .unwrap();
        assert!(load(&path, &saved.source_id).is_err());
        assert!(search(&path, "forged").is_err());
        assert!(save(&path, "Later", "Another valid statement.").is_err());
    }

    #[test]
    fn foreign_schema_refusal_and_empty_read_never_create_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        assert!(list(&path).is_err());
        assert!(!path.exists());
        drop(StudioStore::open(&path).unwrap());
        assert!(list(&path).unwrap().is_empty());
        let conn = Connection::open(&path).unwrap();
        let before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='evidence_sources'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, 0);
        conn.execute_batch("CREATE TABLE unknown_extension(x TEXT)")
            .unwrap();
        assert!(save(&path, "Denied", "A source statement.").is_err());
    }

    #[test]
    fn explicit_deletion_requires_exact_digest_and_never_removes_other_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        drop(StudioStore::open(&path).unwrap());
        let a = save(&path, "Paper A", "Hippocampus stores spatial maps.").unwrap();
        let b = save(&path, "Paper B", "The visual cortex processes contrast.").unwrap();
        assert!(delete(&path, &a.source_id, &"0".repeat(64)).is_err());
        assert!(delete(&path, "invalid", &a.source_sha256).is_err());
        assert!(delete(&path, &b.source_id, &a.source_sha256).is_err());
        assert_eq!(list(&path).unwrap().len(), 2);
        delete(&path, &a.source_id, &a.source_sha256).unwrap();
        assert!(delete(&path, &a.source_id, &a.source_sha256).is_err());
        assert!(load(&path, &a.source_id).is_err());
        assert_eq!(list(&path).unwrap().len(), 1);
        assert_eq!(load(&path, &b.source_id).unwrap().text, b.text);
        assert!(
            search(&path, "hippocampus").unwrap()["results"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            search(&path, "cortex").unwrap()["results"][0]["source_id"],
            b.source_id
        );
        drop(StudioStore::open(&path).expect("deletion must preserve Core schema"));
    }

    #[test]
    fn tampered_corpus_refuses_deletion_and_keeps_all_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        drop(StudioStore::open(&path).unwrap());
        let a = save(&path, "Safe", "Approved research statement.").unwrap();
        let b = save(&path, "Other", "Another research statement.").unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE evidence_sources SET source_text='forged' WHERE source_id=?1",
            [&b.source_id],
        )
        .unwrap();
        assert!(delete(&path, &a.source_id, &a.source_sha256).is_err());
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM evidence_sources", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn excerpt_export_quotes_untrusted_markdown_as_literal_with_hash_and_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        drop(StudioStore::open(&path).unwrap());
        let content = "Hippocampus evidence: ~~~~~~~~~~~\n<script>do_not_execute()</script>";
        let source = save(&path, "Quoted paper", content).unwrap();
        let markdown = export_markdown(&path, "hippocampus").unwrap();
        assert!(markdown.contains(&source.source_sha256));
        assert!(markdown.contains(&source.source_id));
        assert!(markdown.contains(content));
        assert!(markdown.contains("~~~~~~~~~~~~text\n"));
        assert!(markdown.contains("UTF-8 byte range:"));
        assert!(!markdown.contains("model_called"));
        let wrong = export_markdown(&path, "   ");
        assert!(wrong.is_err());
        // Requiring an actual result prevents stale unverified quotes being
        // reused after a deletion.
        delete(&path, &source.source_id, &source.source_sha256).unwrap();
        assert!(
            export_markdown(&path, "hippocampus")
                .unwrap()
                .contains("No matching excerpts.")
        );
    }

    #[test]
    fn independent_delete_connections_have_one_winner_and_cannot_replay() {
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        drop(StudioStore::open(&path).unwrap());
        let source = save(
            &path,
            "Concurrency",
            "A source citation with stable provenance.",
        )
        .unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let path = path.clone();
                let id = source.source_id.clone();
                let hash = source.source_sha256.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    delete(&path, &id, &hash).is_ok()
                })
            })
            .collect::<Vec<_>>();
        let successes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|success| *success)
            .count();
        assert_eq!(
            successes, 1,
            "exactly one SQLite committed deletion may win"
        );
        assert!(list(&path).unwrap().is_empty());
        assert!(StudioStore::open(&path).is_ok());
    }
}
