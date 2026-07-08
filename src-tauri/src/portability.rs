//! Whole-corpus data portability (`docs/data-portability.md`): export the
//! durable analytical stores into one structured, versioned zip archive, and
//! import such an archive back into a store. The load-bearing line is that
//! durable analytical data moves while secrets and machine-local operational
//! state stay behind: the five exported tables (`reports`,
//! `baseline_snapshots`, `vector_memory`, `portfolio_runs`, `holdings_pulls`)
//! plus the report/research files are serialized row-by-row, and `app_settings`
//! (every plaintext credential), the Keychain, `job_runs`, and the telemetry
//! tables are never read — so nothing sensitive can enter the archive.
//!
//! The archive is deliberately **not** a DB-file copy: WAL sidecars would tear,
//! a binary copy cannot strip secrets, and the schema has no version marker to
//! validate against. Import instead runs the target's own idempotent
//! `storage::init_schema` and re-inserts rows, re-deriving each report's
//! machine-specific `markdown_path` from the exported basename. Optional
//! passphrase encryption wraps the finished zip in an AES-256-GCM container
//! keyed by Argon2id; a lost passphrase is unrecoverable by design.
//!
//! Everything here is pure over `(ReportPaths, Path, Option<passphrase>)` — no
//! Tauri types — so the round-trip contract is offline-tested against temp
//! dirs, matching the rest of the deterministic spine.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::pipeline::ReportPaths;
use crate::storage;

/// The archive's own format version, stamped in the manifest. Import rejects an
/// archive newer than it understands; the DB schema itself carries no version
/// marker, which is exactly why the archive stamps one.
pub const FORMAT_VERSION: u32 = 1;

/// Magic prefix of the encrypted container: 8 bytes, then a 16-byte Argon2id
/// salt, a 12-byte AES-GCM nonce, and the ciphertext of the whole zip.
const ENC_MAGIC: &[u8; 8] = b"MSDPENC1";

/// The report namespace's embedder is a fixed cloud model — identical on every
/// machine, which is what makes report vectors portable at all.
const REPORT_EMBEDDER_ID: &str = "text-embedding-3-large";

/// The five exported tables, in insert dependency order (reports first, so the
/// vector summaries and snapshots that join on `report_id` land after them).
const TABLES: [&str; 5] = [
    "reports",
    "baseline_snapshots",
    "vector_memory",
    "portfolio_runs",
    "holdings_pulls",
];

/// The tables' zip entry names, same order — what export writes and import
/// consumes, shared so the two sides can never drift.
const DB_ENTRY_NAMES: [&str; 5] = [
    "db/reports.ndjson",
    "db/baseline_snapshots.ndjson",
    "db/vector_memory.ndjson",
    "db/portfolio_runs.ndjson",
    "db/holdings_pulls.ndjson",
];

// ---------------------------------------------------------------------------
// Archive shapes
// ---------------------------------------------------------------------------

/// `manifest.json` — the self-description at the root of every archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub app_version: String,
    /// Canonical UTC RFC3339 (the frontend renders local time, per the
    /// project-wide date convention).
    pub created_at: String,
    /// Row counts per exported table.
    pub row_counts: BTreeMap<String, u64>,
    /// Durable-learning rows within `vector_memory` — surfaced separately
    /// because the success copy reports learnings, not raw vector rows.
    pub learnings: u64,
    /// Embedder identity per vector-memory namespace, so a future import can
    /// detect a local-embedder mismatch (`docs/data-portability.md §Vector
    /// memory is embedder-bound`). The report namespace is always the fixed
    /// cloud model.
    pub embedders: BTreeMap<String, String>,
    /// Inventory of the file entries (report Markdown + research documents),
    /// checksummed so import can verify against corruption.
    pub files: Vec<FileEntry>,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Zip-relative path, e.g. `reports/2026-07-01-market-signal-report-ab12cd34.md`.
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// One `reports` row on the wire. The machine-specific absolute
/// `markdown_path` is deliberately reduced to its basename here — the archive
/// never carries a source-machine path, and import re-derives the target's own
/// absolute path from this name.
#[derive(Debug, Serialize, Deserialize)]
struct ReportRow {
    report_id: String,
    report_type: String,
    created_at: String,
    risk_posture: String,
    market_cycle: String,
    thesis_stance: String,
    markdown_filename: String,
    summary_json: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SnapshotRow {
    report_id: String,
    captured_at: String,
    schema_version: i64,
    baseline_json: String,
}

/// Auto-increment ids are dropped on export (joins are by `report_id`
/// convention); the embedding BLOB travels as base64 of its little-endian f32
/// bytes.
#[derive(Debug, Serialize, Deserialize)]
struct VectorRow {
    kind: String,
    namespace: String,
    report_id: Option<String>,
    content: String,
    embedding_b64: String,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PortfolioRunRow {
    run_id: String,
    created_at: String,
    run_json: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HoldingsPullRow {
    pulled_at: String,
    holdings_json: String,
}

// ---------------------------------------------------------------------------
// Command-facing results
// ---------------------------------------------------------------------------

/// What an export wrote, for the Settings section's success line.
#[derive(Debug, Clone, Serialize)]
pub struct ExportSummary {
    /// Where the archive landed — surfaced with the counts
    /// (`docs/data-portability.md §Export flow`).
    pub path: String,
    pub reports: u64,
    pub learnings: u64,
    pub snapshots: u64,
    pub portfolio_runs: u64,
    pub holdings_pulls: u64,
    pub files: u64,
    pub encrypted: bool,
}

/// What an import loaded. `skipped_reports` counts report rows whose Markdown
/// body was missing from the archive — skipped with a log line rather than
/// imported as dangling shells.
#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    pub reports: u64,
    pub learnings: u64,
    pub snapshots: u64,
    pub portfolio_runs: u64,
    pub holdings_pulls: u64,
    pub files: u64,
    pub skipped_reports: u64,
}

/// A pre-import read of an archive (and container), for the frontend's
/// confirmation flow.
#[derive(Debug, Clone, Serialize)]
pub struct ArchiveInfo {
    pub encrypted: bool,
    pub format_version: u32,
    pub app_version: String,
    pub created_at: String,
    pub reports: u64,
    pub learnings: u64,
    pub snapshots: u64,
    pub portfolio_runs: u64,
    pub holdings_pulls: u64,
    pub files: u64,
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Build the archive from the store at `paths` and write it to `dest`.
/// `local_embedder_id` stamps the manifest's embedder identity for any
/// local-suite vector namespaces present (the report namespace is always the
/// fixed cloud model); `None` when no local embedder is configured.
pub fn export_archive(
    paths: &ReportPaths,
    dest: &Path,
    passphrase: Option<&str>,
    local_embedder_id: Option<&str>,
) -> Result<ExportSummary> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;

    let reports = read_report_rows(&conn)?;
    let snapshots = read_snapshot_rows(&conn)?;
    let vectors = read_vector_rows(&conn)?;
    let runs = read_portfolio_run_rows(&conn)?;
    let pulls = read_holdings_pull_rows(&conn)?;
    let learnings = vectors.iter().filter(|v| v.kind == "learning").count() as u64;

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for (prefix, dir) in [
        ("reports", &paths.reports_dir),
        ("research-archive", &paths.archive_dir),
        ("research-inbox", &paths.inbox_dir),
    ] {
        for (name, bytes) in dir_files(dir)? {
            files.push((format!("{prefix}/{name}"), bytes));
        }
    }

    let mut row_counts = BTreeMap::new();
    row_counts.insert("reports".to_string(), reports.len() as u64);
    row_counts.insert("baseline_snapshots".to_string(), snapshots.len() as u64);
    row_counts.insert("vector_memory".to_string(), vectors.len() as u64);
    row_counts.insert("portfolio_runs".to_string(), runs.len() as u64);
    row_counts.insert("holdings_pulls".to_string(), pulls.len() as u64);

    let mut embedders = BTreeMap::new();
    embedders.insert("report".to_string(), REPORT_EMBEDDER_ID.to_string());
    let local_namespaces: BTreeSet<&str> = vectors
        .iter()
        .map(|v| v.namespace.as_str())
        .filter(|ns| *ns != "report")
        .collect();
    for ns in local_namespaces {
        if let Some(id) = local_embedder_id {
            embedders.insert(ns.to_string(), id.to_string());
        }
    }

    // The db/*.ndjson entries join the manifest's checksum inventory alongside
    // the store files, so import can verify every entry — table rows included —
    // before its destructive phase.
    let db_payloads: [Vec<u8>; 5] = [
        ndjson(&reports)?,
        ndjson(&snapshots)?,
        ndjson(&vectors)?,
        ndjson(&runs)?,
        ndjson(&pulls)?,
    ];

    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        row_counts,
        learnings,
        embedders,
        files: DB_ENTRY_NAMES
            .iter()
            .copied()
            .zip(db_payloads.iter())
            .chain(files.iter().map(|(path, bytes)| (path.as_str(), bytes)))
            .map(|(path, bytes)| FileEntry {
                path: path.to_string(),
                bytes: bytes.len() as u64,
                sha256: sha256_hex(bytes),
            })
            .collect(),
        encrypted: passphrase.is_some(),
    };

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("manifest.json", opts)?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    for (name, bytes) in DB_ENTRY_NAMES
        .iter()
        .copied()
        .zip(db_payloads.iter())
        .chain(files.iter().map(|(name, bytes)| (name.as_str(), bytes)))
    {
        zip.start_file(name, opts)?;
        zip.write_all(bytes)?;
    }
    let plain = zip.finish()?.into_inner();

    let out = match passphrase {
        Some(p) => encrypt_container(&plain, p)?,
        None => plain,
    };
    std::fs::write(dest, &out).with_context(|| format!("writing archive {dest:?}"))?;

    Ok(ExportSummary {
        path: dest.to_string_lossy().into_owned(),
        reports: reports.len() as u64,
        learnings,
        snapshots: snapshots.len() as u64,
        portfolio_runs: runs.len() as u64,
        holdings_pulls: pulls.len() as u64,
        files: files.len() as u64,
        encrypted: passphrase.is_some(),
    })
}

// ---------------------------------------------------------------------------
// Inspect
// ---------------------------------------------------------------------------

/// Read an archive's manifest without touching the store — the pre-import peek
/// behind the frontend's confirmation flow. An encrypted container without a
/// passphrase is a typed refusal telling the user to supply one.
pub fn inspect_archive(src: &Path, passphrase: Option<&str>) -> Result<ArchiveInfo> {
    let (plain, encrypted) = read_container(src, passphrase)?;
    let mut archive = ZipArchive::new(Cursor::new(plain)).context("reading archive")?;
    let manifest = read_manifest(&mut archive)?;
    check_format_version(&manifest)?;
    Ok(ArchiveInfo {
        encrypted,
        format_version: manifest.format_version,
        app_version: manifest.app_version.clone(),
        created_at: manifest.created_at.clone(),
        reports: count(&manifest, "reports"),
        learnings: manifest.learnings,
        snapshots: count(&manifest, "baseline_snapshots"),
        portfolio_runs: count(&manifest, "portfolio_runs"),
        holdings_pulls: count(&manifest, "holdings_pulls"),
        // Store files only — the db/*.ndjson checksum rows are plumbing, not
        // user-facing corpus files.
        files: manifest
            .files
            .iter()
            .filter(|f| !f.path.starts_with("db/"))
            .count() as u64,
    })
}

/// Whether the store holds any exported-scope data. Import into a non-empty
/// store requires the explicit `replace` confirmation.
pub fn store_is_empty(conn: &Connection) -> Result<bool> {
    for table in TABLES {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        if n > 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Load the archive at `src` into the store at `paths`.
///
/// Fresh-load or replace-all only (merge is deliberately deferred): a non-empty
/// store without `replace` is a refusal, and `replace` clears the five exported
/// tables and the three file stores before loading. `app_settings` is never
/// read or written. The archive is fully validated (format version; every
/// consumed entry manifest-listed with size + checksum verified; every row
/// parsed, its embedding decoded, its filename bare; the schema's uniqueness
/// and cardinality pre-checked) before any destructive step; the row inserts
/// run in one transaction, while the file writes are ordinary filesystem
/// operations — a mid-import I/O failure can leave partial files but never a
/// partially-inserted table.
pub fn import_archive(
    paths: &ReportPaths,
    src: &Path,
    passphrase: Option<&str>,
    replace: bool,
) -> Result<ImportSummary> {
    let (plain, _encrypted) = read_container(src, passphrase)?;
    let mut archive = ZipArchive::new(Cursor::new(plain)).context("reading archive")?;
    let manifest = read_manifest(&mut archive)?;
    check_format_version(&manifest)?;
    let entries = read_entries(&mut archive)?;

    // Verify every manifest-listed file against its size and checksum before
    // touching anything on disk — and never consume bytes the manifest doesn't
    // vouch for: a db/*.ndjson or store-file entry absent from the inventory
    // was never checksummed, so reading it would break the corruption contract.
    // (Unknown entries nothing consumes stay ignored, for additive tolerance.)
    for entry in &manifest.files {
        let Some(bytes) = entries.get(&entry.path) else {
            bail!("archive is missing {} listed in its manifest", entry.path);
        };
        if bytes.len() as u64 != entry.bytes || sha256_hex(bytes) != entry.sha256 {
            bail!("archive entry {} failed its checksum — the file is corrupted", entry.path);
        }
    }
    let verified: BTreeSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    for name in DB_ENTRY_NAMES {
        if entries.contains_key(name) && !verified.contains(name) {
            bail!("archive entry {name} is not listed in the manifest — it cannot be verified");
        }
    }

    let report_rows: Vec<ReportRow> = parse_ndjson(&entries, "db/reports.ndjson")?;
    let snapshot_rows: Vec<SnapshotRow> = parse_ndjson(&entries, "db/baseline_snapshots.ndjson")?;
    let vector_rows: Vec<VectorRow> = parse_ndjson(&entries, "db/vector_memory.ndjson")?;
    let run_rows: Vec<PortfolioRunRow> = parse_ndjson(&entries, "db/portfolio_runs.ndjson")?;
    let pull_rows: Vec<HoldingsPullRow> = parse_ndjson(&entries, "db/holdings_pulls.ndjson")?;

    // Everything the load will need is decoded and checked HERE, before the
    // destructive phase, so a malformed row can only ever abort an import while
    // the target store is still untouched.
    let embeddings: Vec<Vec<u8>> = vector_rows
        .iter()
        .map(|row| {
            B64.decode(&row.embedding_b64)
                .with_context(|| format!("decoding a {} vector-memory embedding", row.kind))
        })
        .collect::<Result<_>>()?;
    for row in &report_rows {
        if !is_bare_filename(&row.markdown_filename) {
            bail!(
                "report {} carries an unsafe markdown filename {:?}",
                row.report_id,
                row.markdown_filename
            );
        }
    }
    // The uniqueness and cardinality the schema will enforce, pre-checked so a
    // constraint violation can only ever abort while the store is untouched —
    // the file mutations precede the row transaction, so an in-transaction
    // failure would strand restored rows pointing at replaced files.
    let mut seen_report_ids = BTreeSet::new();
    for row in &report_rows {
        if !seen_report_ids.insert(row.report_id.as_str()) {
            bail!("archive carries a duplicate report id {:?}", row.report_id);
        }
    }
    let mut seen_run_ids = BTreeSet::new();
    for row in &run_rows {
        if !seen_run_ids.insert(row.run_id.as_str()) {
            bail!("archive carries a duplicate portfolio run id {:?}", row.run_id);
        }
    }
    if pull_rows.len() > 1 {
        bail!(
            "archive carries {} holdings pulls — the store holds at most one",
            pull_rows.len()
        );
    }
    let mut seen_summary_ids = BTreeSet::new();
    for row in vector_rows.iter().filter(|r| r.kind == "summary") {
        if let Some(id) = &row.report_id {
            if !seen_summary_ids.insert(id.as_str()) {
                bail!("archive carries two summary vectors for report {id}");
            }
        }
    }
    // Report rows whose Markdown body is absent from the archive are skipped
    // (never imported as dangling shells); their summary vector rows and
    // baseline snapshots drop with them, mirroring the live store's per-report
    // deletion cascade (`storage::delete_report_baseline_snapshots`). Learnings
    // are kind-based and always survive.
    let archived_report_files: BTreeSet<&str> = entries
        .keys()
        .filter_map(|k| k.strip_prefix("reports/"))
        .collect();
    let skipped: BTreeSet<&str> = report_rows
        .iter()
        .filter(|row| !archived_report_files.contains(row.markdown_filename.as_str()))
        .map(|row| {
            eprintln!(
                "import: skipping report {} — its Markdown body {:?} is missing from the archive",
                row.report_id, row.markdown_filename
            );
            row.report_id.as_str()
        })
        .collect();
    // The store-file write list, prefix-routed and name-checked up front.
    let mut planned_files: Vec<(&std::path::Path, &str, &[u8])> = Vec::new();
    for (name, bytes) in &entries {
        let Some((prefix, rest)) = name.split_once('/') else {
            continue; // manifest.json
        };
        let dir: &std::path::Path = match prefix {
            "reports" => &paths.reports_dir,
            "research-archive" => &paths.archive_dir,
            "research-inbox" => &paths.inbox_dir,
            _ => continue, // db/*.ndjson, or an entry a newer format added
        };
        if !is_bare_filename(rest) {
            bail!("archive entry {name} has an unsafe path");
        }
        if !verified.contains(name.as_str()) {
            bail!("archive entry {name} is not listed in the manifest — it cannot be verified");
        }
        planned_files.push((dir, rest, bytes));
    }

    let mut conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;
    let empty = store_is_empty(&conn)?;
    if !empty && !replace {
        bail!("the store is not empty — import replaces all analytical data and requires explicit confirmation");
    }

    // Destructive phase begins. Files first (clear, then extract), then the
    // rows in a single transaction.
    for dir in [&paths.reports_dir, &paths.archive_dir, &paths.inbox_dir] {
        if !empty {
            clear_dir_files(dir)?;
        }
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating store directory {dir:?}"))?;
    }
    for (dir, name, bytes) in &planned_files {
        std::fs::write(dir.join(name), bytes)
            .with_context(|| format!("writing {name} into the store"))?;
    }

    let tx = conn.transaction()?;
    if !empty {
        for table in TABLES {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
    }
    let mut reports_inserted = 0u64;
    for row in &report_rows {
        if skipped.contains(row.report_id.as_str()) {
            continue;
        }
        // The one path the archive never carries verbatim: re-derived against
        // the *target's* reports dir.
        let markdown_path = paths
            .reports_dir
            .join(&row.markdown_filename)
            .to_string_lossy()
            .into_owned();
        tx.execute(
            "INSERT INTO reports (report_id, report_type, created_at, risk_posture,
                                  market_cycle, thesis_stance, markdown_path, summary_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.report_id,
                row.report_type,
                row.created_at,
                row.risk_posture,
                row.market_cycle,
                row.thesis_stance,
                markdown_path,
                row.summary_json
            ],
        )?;
        reports_inserted += 1;
    }
    let mut snapshots_inserted = 0u64;
    for row in &snapshot_rows {
        if skipped.contains(row.report_id.as_str()) {
            continue;
        }
        tx.execute(
            "INSERT INTO baseline_snapshots (report_id, captured_at, schema_version, baseline_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![row.report_id, row.captured_at, row.schema_version, row.baseline_json],
        )?;
        snapshots_inserted += 1;
    }
    let mut learnings_inserted = 0u64;
    for (row, embedding) in vector_rows.iter().zip(&embeddings) {
        if row.kind == "summary" {
            if let Some(id) = &row.report_id {
                if skipped.contains(id.as_str()) {
                    continue;
                }
            }
        }
        tx.execute(
            "INSERT INTO vector_memory (kind, namespace, report_id, content, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![row.kind, row.namespace, row.report_id, row.content, embedding, row.created_at],
        )?;
        if row.kind == "learning" {
            learnings_inserted += 1;
        }
    }
    for row in &run_rows {
        tx.execute(
            "INSERT INTO portfolio_runs (run_id, created_at, run_json) VALUES (?1, ?2, ?3)",
            params![row.run_id, row.created_at, row.run_json],
        )?;
    }
    for row in &pull_rows {
        tx.execute(
            "INSERT INTO holdings_pulls (id, pulled_at, holdings_json) VALUES (1, ?1, ?2)",
            params![row.pulled_at, row.holdings_json],
        )?;
    }
    tx.commit()?;

    Ok(ImportSummary {
        reports: reports_inserted,
        learnings: learnings_inserted,
        snapshots: snapshots_inserted,
        portfolio_runs: run_rows.len() as u64,
        holdings_pulls: pull_rows.len() as u64,
        files: planned_files.len() as u64,
        skipped_reports: skipped.len() as u64,
    })
}

// ---------------------------------------------------------------------------
// Table serialization
// ---------------------------------------------------------------------------

fn read_report_rows(conn: &Connection) -> Result<Vec<ReportRow>> {
    let mut stmt = conn.prepare(
        "SELECT report_id, report_type, created_at, risk_posture, market_cycle,
                thesis_stance, markdown_path, summary_json
         FROM reports ORDER BY created_at, report_id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(report_id, report_type, created_at, risk_posture, market_cycle, thesis_stance, markdown_path, summary_json)| {
            let markdown_filename = Path::new(&markdown_path)
                .file_name()
                .with_context(|| format!("report {report_id} has a pathless markdown_path {markdown_path:?}"))?
                .to_string_lossy()
                .into_owned();
            Ok(ReportRow {
                report_id,
                report_type,
                created_at,
                risk_posture,
                market_cycle,
                thesis_stance,
                markdown_filename,
                summary_json,
            })
        })
        .collect()
}

fn read_snapshot_rows(conn: &Connection) -> Result<Vec<SnapshotRow>> {
    let mut stmt = conn.prepare(
        "SELECT report_id, captured_at, schema_version, baseline_json
         FROM baseline_snapshots ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SnapshotRow {
                report_id: r.get(0)?,
                captured_at: r.get(1)?,
                schema_version: r.get(2)?,
                baseline_json: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_vector_rows(conn: &Connection) -> Result<Vec<VectorRow>> {
    let mut stmt = conn.prepare(
        "SELECT kind, namespace, report_id, content, embedding, created_at
         FROM vector_memory ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(VectorRow {
                kind: r.get(0)?,
                namespace: r.get(1)?,
                report_id: r.get(2)?,
                content: r.get(3)?,
                embedding_b64: B64.encode(r.get::<_, Vec<u8>>(4)?),
                created_at: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_portfolio_run_rows(conn: &Connection) -> Result<Vec<PortfolioRunRow>> {
    let mut stmt = conn
        .prepare("SELECT run_id, created_at, run_json FROM portfolio_runs ORDER BY id")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PortfolioRunRow {
                run_id: r.get(0)?,
                created_at: r.get(1)?,
                run_json: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_holdings_pull_rows(conn: &Connection) -> Result<Vec<HoldingsPullRow>> {
    let mut stmt =
        conn.prepare("SELECT pulled_at, holdings_json FROM holdings_pulls WHERE id = 1")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(HoldingsPullRow {
                pulled_at: r.get(0)?,
                holdings_json: r.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Container + zip plumbing
// ---------------------------------------------------------------------------

/// Read the file at `src`, transparently decrypting the encrypted container.
/// Returns the plain zip bytes and whether the container was encrypted.
fn read_container(src: &Path, passphrase: Option<&str>) -> Result<(Vec<u8>, bool)> {
    let raw = std::fs::read(src).with_context(|| format!("reading archive {src:?}"))?;
    if raw.starts_with(ENC_MAGIC) {
        let Some(p) = passphrase else {
            bail!("this archive is encrypted — enter its passphrase and try again");
        };
        Ok((decrypt_container(&raw, p)?, true))
    } else {
        Ok((raw, false))
    }
}

fn read_manifest(archive: &mut ZipArchive<Cursor<Vec<u8>>>) -> Result<Manifest> {
    let mut file = archive
        .by_name("manifest.json")
        .context("archive has no manifest.json — not a Market Signal export")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).context("parsing manifest.json")
}

fn check_format_version(manifest: &Manifest) -> Result<()> {
    if manifest.format_version > FORMAT_VERSION {
        bail!(
            "this archive uses format v{} but this build reads up to v{} — update Market Signal to import it",
            manifest.format_version,
            FORMAT_VERSION
        );
    }
    Ok(())
}

fn count(manifest: &Manifest, table: &str) -> u64 {
    manifest.row_counts.get(table).copied().unwrap_or(0)
}

/// All file entries as `name → bytes`, zip-slip-guarded: any entry whose path
/// would escape the archive root is a hard error, never written.
fn read_entries(archive: &mut ZipArchive<Cursor<Vec<u8>>>) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut map = BTreeMap::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }
        if file.enclosed_name().is_none() {
            bail!("archive entry {:?} has an unsafe path", file.name());
        }
        let name = file.name().to_string();
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut bytes)?;
        map.insert(name, bytes);
    }
    Ok(map)
}

fn ndjson<T: Serialize>(rows: &[T]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut out, row)?;
        out.push(b'\n');
    }
    Ok(out)
}

fn parse_ndjson<T: DeserializeOwned>(
    entries: &BTreeMap<String, Vec<u8>>,
    name: &str,
) -> Result<Vec<T>> {
    let Some(bytes) = entries.get(name) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (i, line) in bytes.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        rows.push(
            serde_json::from_slice(line).with_context(|| format!("{name} line {}", i + 1))?,
        );
    }
    Ok(rows)
}

/// Regular, non-hidden files directly inside `dir`, sorted by name for a
/// deterministic archive. A missing directory is an empty store, not an error.
fn dir_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("listing {dir:?}"))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue; // .DS_Store and friends are not corpus data
        }
        names.push(name);
    }
    names.sort();
    for name in names {
        let bytes = std::fs::read(dir.join(&name))?;
        out.push((name, bytes));
    }
    Ok(out)
}

fn clear_dir_files(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("clearing {:?}", entry.path()))?;
        }
    }
    Ok(())
}

/// A bare filename with no path components — the only shape a row's
/// `markdown_filename` or a store file entry may take.
fn is_bare_filename(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name)
            .file_name()
            .map(|f| f == std::ffi::OsStr::new(name))
            .unwrap_or(false)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------
// Passphrase encryption
// ---------------------------------------------------------------------------

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow!("deriving the encryption key: {e}"))?;
    Ok(key)
}

fn encrypt_container(plain: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::rand_core::RngCore;
    use aes_gcm::aead::{Aead, OsRng};
    use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};

    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let key = derive_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|_| anyhow!("encrypting the archive"))?;
    let mut out = Vec::with_capacity(ENC_MAGIC.len() + salt.len() + nonce.len() + ciphertext.len());
    out.extend_from_slice(ENC_MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt_container(raw: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};

    let body = raw
        .strip_prefix(ENC_MAGIC.as_slice())
        .context("not an encrypted Market Signal archive")?;
    if body.len() < 16 + 12 {
        bail!("the encrypted archive is truncated");
    }
    let (salt, rest) = body.split_at(16);
    let (nonce, ciphertext) = rest.split_at(12);
    let key = derive_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    // AES-GCM authenticates, so a bad key and a corrupted body are the same
    // failure — deliberately reported as one message.
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| anyhow!("wrong passphrase, or the archive is corrupted"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector_memory::embedding_to_blob;

    /// A provisioned empty store under a temp dir (guard returned to keep it
    /// alive), mirroring `pipeline`'s tempdir-not-`:memory:` test pattern.
    fn temp_store() -> (tempfile::TempDir, ReportPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = ReportPaths::under(dir.path());
        for d in [&paths.reports_dir, &paths.inbox_dir, &paths.archive_dir] {
            std::fs::create_dir_all(d).unwrap();
        }
        let conn = storage::open(&paths.db_path).unwrap();
        storage::init_schema(&conn).unwrap();
        (dir, paths)
    }

    /// Seed every exported table and file store, plus the `app_settings` secret
    /// that must never reach an archive.
    fn seed_store(paths: &ReportPaths) {
        let conn = storage::open(&paths.db_path).unwrap();
        for (id, filename, body) in [
            ("report-one", "2026-07-01-market-signal-report-report-o.md", "# Issue one\n"),
            ("report-two", "2026-07-05-market-signal-report-report-t.md", "# Issue two\n"),
        ] {
            let md_path = paths.reports_dir.join(filename);
            std::fs::write(&md_path, body).unwrap();
            conn.execute(
                "INSERT INTO reports (report_id, report_type, created_at, risk_posture,
                                      market_cycle, thesis_stance, markdown_path, summary_json)
                 VALUES (?1, 'market_signal', ?2, 'risk-on', 'late-cycle', 'bullish', ?3, '{\"title\":\"t\"}')",
                params![id, format!("2026-07-0{}T12:00:00Z", if id == "report-one" { 1 } else { 5 }), md_path.to_string_lossy()],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO baseline_snapshots (report_id, captured_at, schema_version, baseline_json)
             VALUES ('report-two', '2026-07-05T12:00:00Z', 3, '{\"indices\":{}}')",
            [],
        )
        .unwrap();
        for (kind, namespace, report_id, content, v) in [
            ("summary", "report", Some("report-one"), "Issue one summary.", [0.1f32, 0.2, 0.3]),
            ("learning", "report", None, "Breadth divergences preceded the pullback.", [0.4, 0.5, 0.6]),
            ("learning", "portfolio", None, "A local-suite learning.", [0.7, 0.8, 0.9]),
        ] {
            conn.execute(
                "INSERT INTO vector_memory (kind, namespace, report_id, content, embedding, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, '2026-07-05T12:00:00Z')",
                params![kind, namespace, report_id, content, embedding_to_blob(&v)],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO portfolio_runs (run_id, created_at, run_json)
             VALUES ('run-one', '2026-07-06T12:00:00Z', '{\"verdicts\":[]}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO holdings_pulls (id, pulled_at, holdings_json)
             VALUES (1, '2026-07-06T13:00:00Z', '{\"positions\":[]}')",
            [],
        )
        .unwrap();
        std::fs::write(paths.archive_dir.join("filed-note.md"), "archived research\n").unwrap();
        std::fs::write(paths.inbox_dir.join("pending-note.txt"), "inbox research\n").unwrap();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('openai_api_key', 'sk-SECRET-VALUE')",
            [],
        )
        .unwrap();
    }

    fn table_count(paths: &ReportPaths, table: &str) -> i64 {
        let conn = storage::open(&paths.db_path).unwrap();
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn read_archive_entries(path: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
        let bytes = std::fs::read(path).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        read_entries(&mut archive).unwrap()
    }

    /// Rebuild a zip from an entries map — the tamper tests' common tail.
    fn rebuild_zip(entries: &BTreeMap<String, Vec<u8>>, dest: &std::path::Path) {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in entries {
            zip.start_file(name.as_str(), opts).unwrap();
            zip.write_all(content).unwrap();
        }
        std::fs::write(dest, zip.finish().unwrap().into_inner()).unwrap();
    }

    /// Replace one entry's bytes AND re-stamp its manifest listing (size +
    /// checksum), so the tamper survives verification and only a deeper
    /// validation pass can catch it.
    fn replace_entry_rechecksummed(
        entries: &mut BTreeMap<String, Vec<u8>>,
        name: &str,
        bytes: Vec<u8>,
    ) {
        let mut manifest: Manifest =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        let entry = manifest.files.iter_mut().find(|f| f.path == name).unwrap();
        entry.bytes = bytes.len() as u64;
        entry.sha256 = sha256_hex(&bytes);
        entries.insert(name.to_string(), bytes);
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
    }

    /// One pre-existing report row + Markdown body, so a target store reads as
    /// non-empty and its survival (or clearing) is assertable.
    fn seed_old_report(paths: &ReportPaths) {
        let conn = storage::open(&paths.db_path).unwrap();
        let old_md = paths.reports_dir.join("old-report.md");
        std::fs::write(&old_md, "# Old\n").unwrap();
        conn.execute(
            "INSERT INTO reports (report_id, report_type, created_at, risk_posture,
                                  market_cycle, thesis_stance, markdown_path, summary_json)
             VALUES ('old-report', 'market_signal', '2026-01-01T00:00:00Z', 'mixed',
                     'recovery', 'mixed', ?1, '{}')",
            params![old_md.to_string_lossy()],
        )
        .unwrap();
    }

    #[test]
    fn round_trip_preserves_rows_and_files_and_rederives_paths() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        let summary =
            export_archive(&source, &dest, None, Some("local-embedder-x")).unwrap();
        assert_eq!(summary.path, dest.to_string_lossy());
        assert_eq!(summary.reports, 2);
        assert_eq!(summary.learnings, 2);
        assert_eq!(summary.files, 4); // 2 report bodies + 1 archived + 1 inbox
        assert!(!summary.encrypted);

        let (_b, target) = temp_store();
        let loaded = import_archive(&target, &dest, None, false).unwrap();
        assert_eq!(loaded.reports, 2);
        assert_eq!(loaded.learnings, 2);
        assert_eq!(loaded.snapshots, 1);
        assert_eq!(loaded.portfolio_runs, 1);
        assert_eq!(loaded.holdings_pulls, 1);
        assert_eq!(loaded.skipped_reports, 0);

        let conn = storage::open(&target.db_path).unwrap();
        // markdown_path re-derived against the target's own reports dir, and
        // the body readable through it.
        let markdown_path: String = conn
            .query_row(
                "SELECT markdown_path FROM reports WHERE report_id = 'report-one'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(markdown_path.starts_with(&target.reports_dir.to_string_lossy().into_owned()));
        assert_eq!(std::fs::read_to_string(&markdown_path).unwrap(), "# Issue one\n");
        // Embedding bytes survive the base64 leg exactly.
        let blob: Vec<u8> = conn
            .query_row(
                "SELECT embedding FROM vector_memory WHERE kind = 'summary'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(blob, embedding_to_blob(&[0.1, 0.2, 0.3]));
        // The local-suite namespace row rides along.
        let portfolio_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vector_memory WHERE namespace = 'portfolio'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(portfolio_rows, 1);
        // Research files land in the target's folders.
        assert_eq!(
            std::fs::read_to_string(target.archive_dir.join("filed-note.md")).unwrap(),
            "archived research\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.inbox_dir.join("pending-note.txt")).unwrap(),
            "inbox research\n"
        );
        // Import never touches app_settings — the target's stays empty.
        assert_eq!(table_count(&target, "app_settings"), 0);
    }

    #[test]
    fn archive_never_contains_settings_or_source_paths() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();

        let bytes = std::fs::read(&dest).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let entries = read_entries(&mut archive).unwrap();
        let source_reports_dir = source.reports_dir.to_string_lossy().into_owned();
        for (name, content) in &entries {
            assert!(!name.contains("app_settings"), "entry {name} must not exist");
            let text = String::from_utf8_lossy(content);
            assert!(
                !text.contains("sk-SECRET-VALUE"),
                "entry {name} leaked a credential"
            );
            assert!(
                !text.contains(&source_reports_dir),
                "entry {name} leaked a machine-specific path"
            );
        }
        let manifest: Manifest =
            serde_json::from_slice(entries.get("manifest.json").unwrap()).unwrap();
        assert!(!manifest.row_counts.contains_key("app_settings"));
        assert_eq!(manifest.embedders.get("report").unwrap(), REPORT_EMBEDDER_ID);
        // The db/*.ndjson entries ride the checksum inventory too, so import
        // verifies table rows before its destructive phase.
        for name in [
            "db/reports.ndjson",
            "db/vector_memory.ndjson",
            "db/baseline_snapshots.ndjson",
        ] {
            assert!(
                manifest.files.iter().any(|f| f.path == name),
                "{name} missing from the manifest inventory"
            );
        }
    }

    #[test]
    fn encrypted_round_trip_wrong_passphrase_and_missing_passphrase_fail() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export-enc.zip");
        let summary = export_archive(&source, &dest, Some("hunter2"), None).unwrap();
        assert!(summary.encrypted);
        assert!(std::fs::read(&dest).unwrap().starts_with(ENC_MAGIC));

        let missing = inspect_archive(&dest, None).unwrap_err();
        assert!(missing.to_string().contains("encrypted"), "{missing}");
        let wrong = inspect_archive(&dest, Some("nope")).unwrap_err();
        assert!(wrong.to_string().contains("wrong passphrase"), "{wrong}");

        let info = inspect_archive(&dest, Some("hunter2")).unwrap();
        assert!(info.encrypted);
        assert_eq!(info.reports, 2);

        let (_b, target) = temp_store();
        assert!(import_archive(&target, &dest, Some("nope"), false).is_err());
        let loaded = import_archive(&target, &dest, Some("hunter2"), false).unwrap();
        assert_eq!(loaded.reports, 2);
    }

    #[test]
    fn import_into_nonempty_store_requires_replace_and_replace_clears() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();

        let (_b, target) = temp_store();
        seed_old_report(&target);

        let refused = import_archive(&target, &dest, None, false).unwrap_err();
        assert!(refused.to_string().contains("not empty"), "{refused}");
        assert_eq!(table_count(&target, "reports"), 1);

        let loaded = import_archive(&target, &dest, None, true).unwrap();
        assert_eq!(loaded.reports, 2);
        let conn = storage::open(&target.db_path).unwrap();
        let old: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reports WHERE report_id = 'old-report'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, 0);
        assert!(!target.reports_dir.join("old-report.md").exists());
    }

    #[test]
    fn report_with_missing_markdown_is_skipped_with_its_summary_row() {
        let (_a, source) = temp_store();
        seed_store(&source);
        // Remove one body out-of-band so its row exports without a file.
        std::fs::remove_file(
            source
                .reports_dir
                .join("2026-07-01-market-signal-report-report-o.md"),
        )
        .unwrap();
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();

        let (_b, target) = temp_store();
        let loaded = import_archive(&target, &dest, None, false).unwrap();
        assert_eq!(loaded.skipped_reports, 1);
        assert_eq!(loaded.reports, 1);
        let conn = storage::open(&target.db_path).unwrap();
        // report-one's summary row dropped with it; both learnings survive, and
        // report-two's snapshot is untouched by report-one's skip.
        let summaries: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vector_memory WHERE kind = 'summary'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(summaries, 0);
        assert_eq!(loaded.learnings, 2);
        assert_eq!(loaded.snapshots, 1);
    }

    #[test]
    fn a_skipped_report_cascades_its_baseline_snapshot() {
        let (_a, source) = temp_store();
        seed_store(&source);
        // report-two carries the seeded snapshot; removing ITS body out-of-band
        // exercises the snapshot leg of the skip cascade (mirroring the live
        // store's delete_report_baseline_snapshots).
        std::fs::remove_file(
            source
                .reports_dir
                .join("2026-07-05-market-signal-report-report-t.md"),
        )
        .unwrap();
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();

        let (_b, target) = temp_store();
        let loaded = import_archive(&target, &dest, None, false).unwrap();
        assert_eq!(loaded.skipped_reports, 1);
        assert_eq!(loaded.reports, 1);
        assert_eq!(loaded.snapshots, 0);
        // report-one's summary survives — its report was imported fine.
        let conn = storage::open(&target.db_path).unwrap();
        let summaries: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vector_memory WHERE kind = 'summary'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(summaries, 1);
        assert_eq!(table_count(&target, "baseline_snapshots"), 0);
    }

    #[test]
    fn a_corrupt_embedding_aborts_before_the_destructive_phase() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();

        // Tamper one embedding into JSON-valid but undecodable base64, with a
        // RE-STAMPED manifest listing — so only the pre-destructive decode
        // pass can catch it, not the checksum verification.
        let mut entries = read_archive_entries(&dest);
        let vectors_name = "db/vector_memory.ndjson";
        let tampered_rows: Vec<u8> = {
            let text = String::from_utf8(entries[vectors_name].clone()).unwrap();
            let mut out = Vec::new();
            for (i, line) in text.lines().enumerate() {
                let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
                if i == 0 {
                    v["embedding_b64"] = "@@not-base64@@".into();
                }
                serde_json::to_writer(&mut out, &v).unwrap();
                out.push(b'\n');
            }
            out
        };
        replace_entry_rechecksummed(&mut entries, vectors_name, tampered_rows);
        let tampered_path = source.db_path.parent().unwrap().join("tampered.zip");
        rebuild_zip(&entries, &tampered_path);

        // Replace path — the destructive one. The abort must land before it.
        let (_b, target) = temp_store();
        seed_old_report(&target);
        let err = import_archive(&target, &tampered_path, None, true).unwrap_err();
        assert!(err.to_string().contains("embedding"), "{err}");
        // Old row AND old Markdown body survive: nothing was cleared.
        assert_eq!(table_count(&target, "reports"), 1);
        assert!(target.reports_dir.join("old-report.md").exists());
    }

    #[test]
    fn an_entry_the_manifest_does_not_list_is_never_consumed() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();
        let (_b, target) = temp_store();

        // (a) An extra store file smuggled in without a manifest listing.
        let mut entries = read_archive_entries(&dest);
        entries.insert("reports/rogue.md".to_string(), b"# Rogue\n".to_vec());
        let rogue_path = source.db_path.parent().unwrap().join("rogue.zip");
        rebuild_zip(&entries, &rogue_path);
        let err = import_archive(&target, &rogue_path, None, false).unwrap_err();
        assert!(err.to_string().contains("not listed in the manifest"), "{err}");
        assert_eq!(table_count(&target, "reports"), 0);
        assert!(!target.reports_dir.join("rogue.md").exists());

        // (b) A db entry whose manifest listing was dropped — its bytes were
        // never checksummed, so they must not be parsed.
        let mut entries = read_archive_entries(&dest);
        let mut manifest: Manifest =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest.files.retain(|f| f.path != "db/vector_memory.ndjson");
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let unlisted_path = source.db_path.parent().unwrap().join("unlisted.zip");
        rebuild_zip(&entries, &unlisted_path);
        let err = import_archive(&target, &unlisted_path, None, false).unwrap_err();
        assert!(err.to_string().contains("not listed in the manifest"), "{err}");
        assert_eq!(table_count(&target, "vector_memory"), 0);
    }

    #[test]
    fn duplicate_rows_abort_before_the_destructive_phase() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();

        // Duplicate the first report row, re-stamped so it survives checksum
        // verification — only the uniqueness pre-check can catch it (the INSERT
        // would otherwise violate the PRIMARY KEY mid-transaction, after the
        // file mutations).
        let mut entries = read_archive_entries(&dest);
        let text = String::from_utf8(entries["db/reports.ndjson"].clone()).unwrap();
        let first = text.lines().next().unwrap().to_string();
        let doubled = format!("{first}\n{text}");
        replace_entry_rechecksummed(&mut entries, "db/reports.ndjson", doubled.into_bytes());
        let dup_path = source.db_path.parent().unwrap().join("dup.zip");
        rebuild_zip(&entries, &dup_path);

        // Replace path — the destructive one. The abort must land before it.
        let (_b, target) = temp_store();
        seed_old_report(&target);
        let err = import_archive(&target, &dup_path, None, true).unwrap_err();
        assert!(err.to_string().contains("duplicate report id"), "{err}");
        assert_eq!(table_count(&target, "reports"), 1);
        assert!(target.reports_dir.join("old-report.md").exists());
    }

    #[test]
    fn inspect_reads_manifest_counts_and_store_is_empty_tracks_seeding() {
        let (_a, source) = temp_store();
        {
            let conn = storage::open(&source.db_path).unwrap();
            assert!(store_is_empty(&conn).unwrap());
        }
        seed_store(&source);
        {
            let conn = storage::open(&source.db_path).unwrap();
            assert!(!store_is_empty(&conn).unwrap());
        }
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();
        let info = inspect_archive(&dest, None).unwrap();
        assert!(!info.encrypted);
        assert_eq!(info.format_version, FORMAT_VERSION);
        assert_eq!(info.reports, 2);
        assert_eq!(info.learnings, 2);
        assert_eq!(info.snapshots, 1);
        assert_eq!(info.portfolio_runs, 1);
        assert_eq!(info.holdings_pulls, 1);
        assert_eq!(info.files, 4);
    }

    #[test]
    fn corrupted_file_entry_fails_its_checksum() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None, None).unwrap();

        // Rebuild the zip with one report body tampered, manifest untouched.
        let mut entries = read_archive_entries(&dest);
        let name = entries
            .keys()
            .find(|k| k.starts_with("reports/") && k.contains("report-o"))
            .unwrap()
            .clone();
        entries.insert(name, b"tampered".to_vec());
        let tampered_path = source.db_path.parent().unwrap().join("tampered.zip");
        rebuild_zip(&entries, &tampered_path);

        let (_b, target) = temp_store();
        let err = import_archive(&target, &tampered_path, None, false).unwrap_err();
        assert!(err.to_string().contains("checksum"), "{err}");
        // Validation happens before any destructive step: target untouched.
        assert_eq!(table_count(&target, "reports"), 0);
    }
}
