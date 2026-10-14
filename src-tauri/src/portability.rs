//! Whole-corpus data portability (`docs/data-portability.md`): export the durable
//! analytical stores into one versioned zip archive and import one back. Durable
//! analytical data moves; `app_settings`, the Keychain, `job_runs` and the telemetry
//! tables are never read, so nothing sensitive can enter the archive.

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
/// marker, which is why the archive stamps one. Bump it whenever a table joins the
/// archive or a persisted row shape changes, so an older build refuses the archive
/// instead of misreading it (`docs/data-portability.md` §Build-order placement).
/// Every pre-release stamp below it is refused outright (`check_format_version`);
/// a v1 archive, the shipped build's format, imports complete under its own
/// five-entry set (`required_db_entries`).
pub const FORMAT_VERSION: u32 = 24;

/// Magic prefix of the encrypted container: 8 bytes, then a 16-byte Argon2id
/// salt, a 12-byte AES-GCM nonce, and the ciphertext of the whole zip.
const ENC_MAGIC: &[u8; 8] = b"MSDPENC1";

/// The twelve exported tables, in insert dependency order (reports first, so
/// the vector summaries and snapshots that join on `report_id` land after them;
/// episodes before the checks that reference them).
const TABLES: [&str; 12] = [
    "reports",
    "baseline_snapshots",
    "vector_memory",
    "portfolio_runs",
    "holdings_pulls",
    "portfolio_quick_checks",
    "portfolio_episodes",
    "portfolio_episode_checks",
    "price_bars",
    "web_documents",
    "web_source_state",
    "factor_observations",
];

/// The tables' zip entry names, same order — what export writes and import
/// consumes, shared so the two sides can never drift.
const DB_ENTRY_NAMES: [&str; 12] = [
    "db/reports.ndjson",
    "db/baseline_snapshots.ndjson",
    "db/vector_memory.ndjson",
    "db/portfolio_runs.ndjson",
    "db/holdings_pulls.ndjson",
    "db/portfolio_quick_checks.ndjson",
    "db/portfolio_episodes.ndjson",
    "db/portfolio_episode_checks.ndjson",
    "db/price_bars.ndjson",
    "db/web_documents.ndjson",
    "db/web_source_state.ndjson",
    "db/factor_observations.ndjson",
];

/// The db entries an archive's own format version requires (`docs/data-portability.md`
/// §Import flow): a v1 archive, the shipped build's format, is complete at five
/// entries, and requiring the current set of it would refuse it as truncated.
fn required_db_entries(format_version: u32) -> &'static [&'static str] {
    if format_version >= FORMAT_VERSION {
        &DB_ENTRY_NAMES
    } else {
        &DB_ENTRY_NAMES[..5]
    }
}

// ---------------------------------------------------------------------------
// Archive shapes
// ---------------------------------------------------------------------------

/// `manifest.json` — the self-description at the root of every archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub app_version: String,
    /// UTC RFC3339; the frontend renders local time.
    pub created_at: String,
    pub row_counts: BTreeMap<String, u64>,
    /// Durable-learning rows within `vector_memory`, surfaced separately because
    /// the success copy reports learnings, not raw vector rows.
    pub learnings: u64,
    /// The vector rows' embedder identity by namespace — the report's fixed cloud
    /// model, which is what makes the vectors portable
    /// (`docs/data-portability.md §Vector memory is embedder-bound`).
    pub embedders: BTreeMap<String, String>,
    /// Checksummed inventory of every entry, so import can verify against corruption.
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

/// One `reports` row on the wire. The machine-specific `markdown_path` is reduced
/// to its basename: the archive never carries a source-machine path, and import
/// re-derives the target's own path from the name.
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

/// The single-row quick-check store on the wire (`docs/portfolio-analysis.md`
/// §The quick check); its flags and events do not regenerate on the next sweep,
/// so it moves with the corpus.
#[derive(Debug, Serialize, Deserialize)]
struct QuickCheckRow {
    checked_at: String,
    state_json: String,
}

/// One episode on the wire (`docs/portfolio-analysis.md` §Outcome learning), carried
/// under its id: an aged-out run cannot regenerate its episodes, and the checks
/// reference the id.
#[derive(Debug, Serialize, Deserialize)]
struct EpisodeRow {
    id: i64,
    symbol: String,
    created_on: String,
    episode_json: String,
}

/// One check on the wire, under its id (run order) and its episode's.
#[derive(Debug, Serialize, Deserialize)]
struct EpisodeCheckRow {
    id: i64,
    episode_id: i64,
    horizon: String,
    run_id: String,
    check_json: String,
}

/// One price-bar cache row on the wire (`docs/storage.md §Local Analysis Suite
/// Storage`), carried so imported pending episodes can mature offline.
#[derive(Debug, Serialize, Deserialize)]
struct PriceBarRow {
    symbol: String,
    date: String,
    close: f64,
}

/// One web-research document-cache row on the wire (`docs/storage.md §Local
/// Analysis Suite Storage`).
#[derive(Debug, Serialize, Deserialize)]
struct WebDocumentRow {
    /// Normalized requested-URL cache key.
    url: String,
    /// Normalized post-redirect provenance.
    final_url: String,
    host: String,
    retrieved_at: String,
    title: String,
    text: String,
    extraction_quality: f64,
    thin_stub: bool,
}

/// One web-research source-state row on the wire: a domain's learned extraction
/// telemetry.
#[derive(Debug, Serialize, Deserialize)]
struct WebSourceStateRow {
    host: String,
    full_count: i64,
    thin_count: i64,
    failed_count: i64,
    denied_count: i64,
    profile: Option<String>,
    render_first: bool,
    updated_at: String,
}

/// One factor-distribution store row on the wire (`docs/storage.md §Local
/// Analysis Suite Storage`); `value` is absent exactly when the reading is floored.
#[derive(Debug, Serialize, Deserialize)]
struct FactorObservationRow {
    issuer: String,
    factor: String,
    sector: String,
    cap_band: String,
    #[serde(deserialize_with = "crate::portfolio::required_option")]
    value: Option<f64>,
    floored: bool,
    observed_on: String,
}

// ---------------------------------------------------------------------------
// Command-facing results
// ---------------------------------------------------------------------------

/// What an export wrote, for the Settings section's success line.
#[derive(Debug, Clone, Serialize)]
pub struct ExportSummary {
    /// Surfaced with the counts (`docs/data-portability.md §Export flow`).
    pub path: String,
    pub reports: u64,
    pub learnings: u64,
    pub snapshots: u64,
    pub portfolio_runs: u64,
    pub holdings_pulls: u64,
    pub files: u64,
    pub encrypted: bool,
}

/// What an import loaded. `skipped_reports` counts report rows whose Markdown body
/// was missing from the archive, skipped rather than imported as dangling shells.
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

/// A pre-import read of an archive, for the frontend's confirmation flow.
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
pub fn export_archive(
    paths: &ReportPaths,
    dest: &Path,
    passphrase: Option<&str>,
) -> Result<ExportSummary> {
    let conn = storage::open(&paths.db_path)?;
    storage::init_schema(&conn)?;

    let reports = read_report_rows(&conn)?;
    let snapshots = read_snapshot_rows(&conn)?;
    let vectors = read_vector_rows(&conn)?;
    let runs = read_portfolio_run_rows(&conn)?;
    let pulls = read_holdings_pull_rows(&conn)?;
    let quick_checks = read_quick_check_rows(&conn)?;
    let episodes = read_episode_rows(&conn)?;
    let episode_checks = read_episode_check_rows(&conn)?;
    let price_bars = read_price_bar_rows(&conn)?;
    let web_documents = read_web_document_rows(&conn)?;
    let web_source_states = read_web_source_state_rows(&conn)?;
    let factor_observations = read_factor_observation_rows(&conn)?;
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
    row_counts.insert("portfolio_quick_checks".to_string(), quick_checks.len() as u64);
    row_counts.insert("portfolio_episodes".to_string(), episodes.len() as u64);
    row_counts.insert(
        "portfolio_episode_checks".to_string(),
        episode_checks.len() as u64,
    );
    row_counts.insert("price_bars".to_string(), price_bars.len() as u64);
    row_counts.insert("web_documents".to_string(), web_documents.len() as u64);
    row_counts.insert(
        "web_source_state".to_string(),
        web_source_states.len() as u64,
    );
    row_counts.insert(
        "factor_observations".to_string(),
        factor_observations.len() as u64,
    );

    let mut embedders = BTreeMap::new();
    embedders.insert(
        "report".to_string(),
        crate::embedding::EMBEDDING_MODEL.to_string(),
    );

    // The db/*.ndjson entries join the checksum inventory alongside the store files,
    // so import can verify table rows too before its destructive phase.
    let db_payloads: [Vec<u8>; 12] = [
        ndjson(&reports)?,
        ndjson(&snapshots)?,
        ndjson(&vectors)?,
        ndjson(&runs)?,
        ndjson(&pulls)?,
        ndjson(&quick_checks)?,
        ndjson(&episodes)?,
        ndjson(&episode_checks)?,
        ndjson(&price_bars)?,
        ndjson(&web_documents)?,
        ndjson(&web_source_states)?,
        ndjson(&factor_observations)?,
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
    // Write-then-rename, so a failed write (disk full, unplugged volume) can never
    // leave a truncated archive sitting at the chosen path looking like a backup.
    let tmp = match dest.file_name() {
        Some(name) => dest.with_file_name(format!("{}.partial", name.to_string_lossy())),
        None => bail!("export destination {dest:?} has no file name"),
    };
    if let Err(e) = std::fs::write(&tmp, &out) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing archive {tmp:?}"));
    }
    if let Err(e) = std::fs::rename(&tmp, dest) {
        // Don't strand the `.partial` sibling.
        let _ = std::fs::remove_file(&tmp);
        return Err(e)
            .with_context(|| format!("moving the finished archive into place at {dest:?}"));
    }

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

/// Read an archive's manifest without touching the store. An encrypted container
/// without a passphrase is a refusal telling the user to supply one.
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
        // Store files only; the db/*.ndjson rows are plumbing, not corpus files.
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

/// Load the archive at `src` into the store at `paths`. Fresh-load or replace-all
/// only (merge is deliberately deferred): a non-empty store without `replace` is a
/// refusal. The archive is fully validated before any destructive step; the row
/// inserts run in one transaction while the file writes are ordinary filesystem
/// operations, so a mid-import I/O failure can leave partial files but never a
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

    // Verify every manifest-listed entry before touching anything on disk, and never
    // consume bytes the manifest doesn't vouch for: an unlisted entry was never
    // checksummed. Unknown entries nothing consumes stay ignored.
    for entry in &manifest.files {
        let Some(bytes) = entries.get(&entry.path) else {
            bail!("archive is missing {} listed in its manifest", entry.path);
        };
        if bytes.len() as u64 != entry.bytes || sha256_hex(bytes) != entry.sha256 {
            bail!("archive entry {} failed its checksum — the file is corrupted", entry.path);
        }
    }
    let verified: BTreeSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    // An export always writes its version's full entry set, so a missing required
    // entry is truncation or tampering, not a sparse store; without this check a
    // dropped entry+listing pair would import the table as zero rows (`parse_ndjson`
    // tolerates absence, which is how a v1 archive's later entries read as empty).
    for name in required_db_entries(manifest.format_version) {
        if !verified.contains(name) {
            bail!("archive entry {name} is missing or not listed in the manifest — it cannot be verified");
        }
    }
    // Every table entry present must be manifest-vouched whatever the version
    // requires: a later format's entry riding an older archive unlisted was never
    // checksummed, so it is refused rather than parsed.
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
    let quick_rows: Vec<QuickCheckRow> = parse_ndjson(&entries, "db/portfolio_quick_checks.ndjson")?;
    let episode_rows: Vec<EpisodeRow> = parse_ndjson(&entries, "db/portfolio_episodes.ndjson")?;
    let episode_check_rows: Vec<EpisodeCheckRow> =
        parse_ndjson(&entries, "db/portfolio_episode_checks.ndjson")?;
    let price_bar_rows: Vec<PriceBarRow> = parse_ndjson(&entries, "db/price_bars.ndjson")?;
    let web_document_rows: Vec<WebDocumentRow> = parse_ndjson(&entries, "db/web_documents.ndjson")?;
    let web_source_state_rows: Vec<WebSourceStateRow> =
        parse_ndjson(&entries, "db/web_source_state.ndjson")?;
    let factor_observation_rows: Vec<FactorObservationRow> =
        parse_ndjson(&entries, "db/factor_observations.ndjson")?;

    // Everything the load needs is decoded and checked here, before the destructive
    // phase, so a malformed row can only abort while the target is untouched.
    // Every vector row is the report's (`docs/storage.md §Local Vector Memory`).
    if let Some(row) = vector_rows.iter().find(|r| r.namespace != "report") {
        bail!(
            "archive carries a vector-memory row in the {:?} namespace — only the \
             report's rows are portable",
            row.namespace
        );
    }
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
    // The schema's uniqueness and cardinality, pre-checked: the file mutations
    // precede the row transaction, so an in-transaction constraint failure would
    // strand replaced files with no rows pointing at them.
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
    if quick_rows.len() > 1 {
        bail!(
            "archive carries {} quick-check rows — the store holds at most one",
            quick_rows.len()
        );
    }
    // The episode store's and price-bar cache's schema constraints, mirrored.
    let mut seen_episode_ids = BTreeSet::new();
    for row in &episode_rows {
        if !seen_episode_ids.insert(row.id) {
            bail!("archive carries a duplicate episode id {}", row.id);
        }
    }
    let mut seen_check_ids = BTreeSet::new();
    let mut seen_check_horizons = BTreeSet::new();
    for row in &episode_check_rows {
        if !seen_check_ids.insert(row.id) {
            bail!("archive carries a duplicate check id {}", row.id);
        }
        if !seen_check_horizons.insert((row.episode_id, row.horizon.as_str())) {
            bail!(
                "archive carries two {} checks for episode {}",
                row.horizon,
                row.episode_id
            );
        }
        if !seen_episode_ids.contains(&row.episode_id) {
            bail!(
                "archive carries check {} for episode {}, which it does not carry",
                row.id,
                row.episode_id
            );
        }
    }
    let mut seen_bar_keys = BTreeSet::new();
    for row in &price_bar_rows {
        if !seen_bar_keys.insert((row.symbol.as_str(), row.date.as_str())) {
            bail!(
                "archive carries a duplicate price bar for {} on {}",
                row.symbol,
                row.date
            );
        }
    }
    // The web-research stores' primary keys, mirrored.
    let mut seen_doc_urls = BTreeSet::new();
    for row in &web_document_rows {
        if !seen_doc_urls.insert(row.url.as_str()) {
            bail!("archive carries a duplicate cached document for {:?}", row.url);
        }
    }
    let mut seen_state_hosts = BTreeSet::new();
    for row in &web_source_state_rows {
        if !seen_state_hosts.insert(row.host.as_str()) {
            bail!("archive carries a duplicate source state for {:?}", row.host);
        }
    }
    // The factor store's (issuer, factor) key and its value-or-floored check, mirrored.
    let mut seen_observation_keys = BTreeSet::new();
    for row in &factor_observation_rows {
        if !seen_observation_keys.insert((row.issuer.as_str(), row.factor.as_str())) {
            bail!(
                "archive carries a duplicate {} observation for {}",
                row.factor,
                row.issuer
            );
        }
        if row.value.is_some() == row.floored {
            bail!(
                "archive carries a {} observation for {} that is both or neither a value and floored",
                row.factor,
                row.issuer
            );
        }
    }
    let mut seen_summary_ids = BTreeSet::new();
    for row in vector_rows.iter().filter(|r| r.kind == "summary") {
        if let Some(id) = &row.report_id {
            if !seen_summary_ids.insert(id.as_str()) {
                bail!("archive carries two summary vectors for report {id}");
            }
        }
    }
    // A report row whose Markdown body is absent is skipped, and its summary vector
    // and baseline snapshot drop with it, mirroring the live store's per-report
    // cascade (`storage::delete_report_baseline_snapshots`); learnings always survive.
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

    // Destructive phase: files first, then the rows in a single transaction.
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
        // Re-derived against the target's reports dir; the archive carries no path.
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
            "INSERT INTO portfolio_runs (run_id, created_at, run_json) \
             VALUES (?1, ?2, ?3)",
            params![row.run_id, row.created_at, row.run_json],
        )?;
    }
    for row in &pull_rows {
        tx.execute(
            "INSERT INTO holdings_pulls (id, pulled_at, holdings_json) VALUES (1, ?1, ?2)",
            params![row.pulled_at, row.holdings_json],
        )?;
    }
    for row in &quick_rows {
        tx.execute(
            "INSERT INTO portfolio_quick_checks (id, checked_at, state_json) VALUES (1, ?1, ?2)",
            params![row.checked_at, row.state_json],
        )?;
    }
    for row in &episode_rows {
        tx.execute(
            "INSERT INTO portfolio_episodes (id, symbol, created_on, episode_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![row.id, row.symbol, row.created_on, row.episode_json],
        )?;
    }
    for row in &episode_check_rows {
        tx.execute(
            "INSERT INTO portfolio_episode_checks (id, episode_id, horizon, run_id, check_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![row.id, row.episode_id, row.horizon, row.run_id, row.check_json],
        )?;
    }
    for row in &price_bar_rows {
        tx.execute(
            "INSERT INTO price_bars (symbol, date, close) VALUES (?1, ?2, ?3)",
            params![row.symbol, row.date, row.close],
        )?;
    }
    for row in &web_document_rows {
        tx.execute(
            "INSERT INTO web_documents (url, final_url, host, retrieved_at, title, text,
                                        extraction_quality, thin_stub)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.url,
                row.final_url,
                row.host,
                row.retrieved_at,
                row.title,
                row.text,
                row.extraction_quality,
                row.thin_stub as i64
            ],
        )?;
    }
    for row in &web_source_state_rows {
        tx.execute(
            "INSERT INTO web_source_state (host, full_count, thin_count, failed_count,
                                           denied_count, profile, render_first,
                                           updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.host,
                row.full_count,
                row.thin_count,
                row.failed_count,
                row.denied_count,
                row.profile,
                row.render_first as i64,
                row.updated_at
            ],
        )?;
    }
    for row in &factor_observation_rows {
        tx.execute(
            "INSERT INTO factor_observations (issuer, factor, sector, cap_band, value,
                                              floored, observed_on)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.issuer,
                row.factor,
                row.sector,
                row.cap_band,
                row.value,
                row.floored as i64,
                row.observed_on
            ],
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
         FROM vector_memory WHERE namespace = 'report' ORDER BY id",
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

fn read_quick_check_rows(conn: &Connection) -> Result<Vec<QuickCheckRow>> {
    let mut stmt =
        conn.prepare("SELECT checked_at, state_json FROM portfolio_quick_checks WHERE id = 1")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(QuickCheckRow {
                checked_at: r.get(0)?,
                state_json: r.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_episode_rows(conn: &Connection) -> Result<Vec<EpisodeRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, symbol, created_on, episode_json FROM portfolio_episodes ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(EpisodeRow {
                id: r.get(0)?,
                symbol: r.get(1)?,
                created_on: r.get(2)?,
                episode_json: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_episode_check_rows(conn: &Connection) -> Result<Vec<EpisodeCheckRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, episode_id, horizon, run_id, check_json
         FROM portfolio_episode_checks ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(EpisodeCheckRow {
                id: r.get(0)?,
                episode_id: r.get(1)?,
                horizon: r.get(2)?,
                run_id: r.get(3)?,
                check_json: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_web_document_rows(conn: &Connection) -> Result<Vec<WebDocumentRow>> {
    let mut stmt = conn.prepare(
        "SELECT url, final_url, host, retrieved_at, title, text, extraction_quality, thin_stub
         FROM web_documents ORDER BY url",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebDocumentRow {
                url: r.get(0)?,
                final_url: r.get(1)?,
                host: r.get(2)?,
                retrieved_at: r.get(3)?,
                title: r.get(4)?,
                text: r.get(5)?,
                extraction_quality: r.get(6)?,
                thin_stub: r.get::<_, i64>(7)? != 0,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_web_source_state_rows(conn: &Connection) -> Result<Vec<WebSourceStateRow>> {
    let mut stmt = conn.prepare(
        "SELECT host, full_count, thin_count, failed_count, denied_count, profile,
                render_first, updated_at
         FROM web_source_state ORDER BY host",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebSourceStateRow {
                host: r.get(0)?,
                full_count: r.get(1)?,
                thin_count: r.get(2)?,
                failed_count: r.get(3)?,
                denied_count: r.get(4)?,
                profile: r.get(5)?,
                render_first: r.get::<_, i64>(6)? != 0,
                updated_at: r.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_factor_observation_rows(conn: &Connection) -> Result<Vec<FactorObservationRow>> {
    let mut stmt = conn.prepare(
        "SELECT issuer, factor, sector, cap_band, value, floored, observed_on
         FROM factor_observations ORDER BY issuer, factor",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(FactorObservationRow {
                issuer: r.get(0)?,
                factor: r.get(1)?,
                sector: r.get(2)?,
                cap_band: r.get(3)?,
                value: r.get(4)?,
                floored: r.get(5)?,
                observed_on: r.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_price_bar_rows(conn: &Connection) -> Result<Vec<PriceBarRow>> {
    let mut stmt =
        conn.prepare("SELECT symbol, date, close FROM price_bars ORDER BY symbol, date")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PriceBarRow {
                symbol: r.get(0)?,
                date: r.get(1)?,
                close: r.get(2)?,
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

/// Ceiling on `manifest.json` itself: it is inflated before `read_entries_bounded`
/// can apply the archive-wide ceiling, so a bomb in the manifest would bypass that
/// bound. A real manifest is a few KB of inventory.
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

fn read_manifest(archive: &mut ZipArchive<Cursor<Vec<u8>>>) -> Result<Manifest> {
    read_manifest_bounded(archive, MAX_MANIFEST_BYTES)
}

/// `read_manifest` with the ceiling injectable for tests.
fn read_manifest_bounded(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    limit: u64,
) -> Result<Manifest> {
    let mut file = archive
        .by_name("manifest.json")
        .context("archive has no manifest.json — not a Market Signal export")?;
    let mut bytes = Vec::new();
    (&mut file).take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!(
            "manifest.json unpacks past the {} MB ceiling — refusing to read it into memory",
            limit / (1024 * 1024)
        );
    }
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
    if matches!(manifest.format_version, 0 | 2..=23) {
        bail!(
            "this archive uses format v{} — a pre-release format no shipped build wrote, which this build no longer reads",
            manifest.format_version
        );
    }
    Ok(())
}

fn count(manifest: &Manifest, table: &str) -> u64 {
    manifest.row_counts.get(table).copied().unwrap_or(0)
}

/// Ceiling on an archive's total unpacked size. The whole archive is held in memory
/// (a real corpus is tens of MB), so a crafted zip must not be able to demand
/// arbitrary memory: deflate inflates up to ~1032:1, and an entry's header can lie
/// about its size.
const MAX_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// All file entries as `name → bytes`, zip-slip-guarded: an entry whose path would
/// escape the archive root is a hard error.
fn read_entries(archive: &mut ZipArchive<Cursor<Vec<u8>>>) -> Result<BTreeMap<String, Vec<u8>>> {
    read_entries_bounded(archive, MAX_UNPACKED_BYTES)
}

/// `read_entries` with the ceiling injectable for tests. Both guards distrust the
/// entry header: preallocation is capped, and the ceiling is enforced on bytes
/// actually inflated, via a `take` one past the remaining budget.
fn read_entries_bounded(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    limit: u64,
) -> Result<BTreeMap<String, Vec<u8>>> {
    const PREALLOC_CAP: u64 = 16 * 1024 * 1024;
    let mut remaining = limit;
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
        let mut bytes = Vec::with_capacity(file.size().min(PREALLOC_CAP) as usize);
        (&mut file).take(remaining.saturating_add(1)).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > remaining {
            bail!(
                "archive unpacks past the {} MB ceiling — refusing to read it into memory",
                limit / (1024 * 1024)
            );
        }
        remaining -= bytes.len() as u64;
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

/// The only shape a row's `markdown_filename` or a store file entry may take.
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

/// The Argon2id cost parameters, FROZEN rather than defaulted: the container stores
/// salt + nonce but no KDF parameters, so the derivation must be bit-stable forever.
/// `Params::DEFAULT` is a crate-version artifact (argon2 0.4→0.5 moved m/t), and
/// inheriting a bump would make every existing encrypted archive fail as "wrong
/// passphrase" with no recovery path. These are argon2 0.5's Argon2id v19 defaults
/// (m in KiB). Raising them requires a new container version (`ENC_MAGIC`).
const ARGON2_M_COST: u32 = 19_456;
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let params = argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
        .map_err(|e| anyhow!("building the key-derivation parameters: {e}"))?;
    let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
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

    /// A provisioned empty store under a temp dir; the guard keeps it alive.
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
            ("learning", "report", None, "Price-led rallies faded into the print.", [0.7, 0.8, 0.9]),
        ] {
            conn.execute(
                "INSERT INTO vector_memory (kind, namespace, report_id, content, embedding, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, '2026-07-05T12:00:00Z')",
                params![kind, namespace, report_id, content, embedding_to_blob(&v)],
            )
            .unwrap();
        }
        // The runs carry every field of the current run shape, so the round trip
        // proves they decode whole.
        for (run_id, created_at) in [
            ("run-one", "2026-07-06T12:00:00Z"),
            ("run-two", "2026-07-06T14:00:00Z"),
        ] {
            crate::portfolio::store::insert_run(
                &conn,
                &crate::portfolio::store::archive_test_run(run_id, created_at),
            )
            .unwrap();
        }
        crate::portfolio::store::save_pull(
            &conn,
            &crate::portfolio::store::HoldingsPull {
                pulled_at: "2026-07-06T13:00:00Z".into(),
                holdings: crate::portfolio::store::archive_test_run("pull", "x").holdings,
            },
        )
        .unwrap();
        crate::portfolio::store::save_quick_check(
            &conn,
            &crate::portfolio::quick_check::QuickCheckState {
                parameter_version: crate::portfolio::quick_check::QUICK_CHECK_PARAMETER_VERSION
                    .into(),
                swept_run_id: "run-one".into(),
                last_checked_at: "2026-07-07T09:00:00Z".into(),
                rate_cache: None,
                holdings: vec![],
            },
        )
        .unwrap();
        // The episode and its check under ids the store never assigned, so
        // the round trip shows they travel with the rows.
        let (record, check) = crate::portfolio::store::archive_test_episode();
        conn.execute(
            "INSERT INTO portfolio_episodes (id, symbol, created_on, episode_json)
             VALUES (7, ?1, ?2, ?3)",
            params![record.symbol, record.created_on, serde_json::to_string(&record).unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO portfolio_episode_checks (id, episode_id, horizon, run_id, check_json)
             VALUES (3, 7, ?1, ?2, ?3)",
            params![check.horizon.key(), check.run_id, serde_json::to_string(&check).unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO price_bars (symbol, date, close) VALUES ('AAPL', '2026-07-03', 195.25)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO web_documents (url, final_url, host, retrieved_at, title, text,
                                        extraction_quality, thin_stub)
             VALUES ('https://reuters.com/go-widget', 'https://reuters.com/widget',
                     'reuters.com', '2026-07-06T10:00:00+00:00', 'Widget beats',
                     'Widget Co reported…', 0.9, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO web_source_state (host, full_count, thin_count, failed_count,
                                           denied_count, profile, render_first,
                                           updated_at)
             VALUES ('bloomberg.com', 0, 3, 2, 1, 'js_required', 1,
                     '2026-07-06T10:00:00+00:00')",
            [],
        )
        .unwrap();
        // A finite reading and a floored one, so both shapes travel.
        conn.execute(
            "INSERT INTO factor_observations (issuer, factor, sector, cap_band, value,
                                              floored, observed_on)
             VALUES ('AAPL', 'earnings-yield', 'Technology', 'mega', 0.031, 0, '2026-07-06'),
                    ('AAPL', 'debt-to-equity', 'Technology', 'mega', NULL, 1, '2026-07-06')",
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

    /// Every row of `table`, every column but `except`, in column order — the
    /// round trip's full-row parity read.
    fn table_rows(
        paths: &ReportPaths,
        table: &str,
        except: &[&str],
    ) -> Vec<Vec<rusqlite::types::Value>> {
        let conn = storage::open(&paths.db_path).unwrap();
        let columns: Vec<String> = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(Result::unwrap)
            .filter(|c| !except.contains(&c.as_str()))
            .collect();
        let order: Vec<String> = (1..=columns.len()).map(|i| i.to_string()).collect();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM {table} ORDER BY {}",
                columns.join(", "),
                order.join(", ")
            ))
            .unwrap();
        stmt.query_map([], |r| {
            (0..columns.len())
                .map(|i| r.get::<_, rusqlite::types::Value>(i))
                .collect()
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
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

    /// Add a new entry AND stamp a fresh manifest listing for it, so the
    /// addition survives checksum verification and only the path checks can
    /// catch it.
    fn add_entry_rechecksummed(
        entries: &mut BTreeMap<String, Vec<u8>>,
        name: &str,
        bytes: Vec<u8>,
    ) {
        let mut manifest: Manifest =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest.files.push(FileEntry {
            path: name.to_string(),
            bytes: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
        entries.insert(name.to_string(), bytes);
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
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
    fn entry3_archive_round_trip_preserves_the_run_audits_research_record() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let conn = storage::open(&source.db_path).unwrap();
        let run = crate::portfolio::store::entry3_test_run();
        crate::portfolio::store::insert_run(&conn, &run).unwrap();
        let dest = source.db_path.parent().unwrap().join("dates.zip");
        export_archive(&source, &dest, None).unwrap();
        let (_b, target) = temp_store();
        import_archive(&target, &dest, None, false).unwrap();
        let conn = storage::open(&target.db_path).unwrap();
        let restored = crate::portfolio::store::run_by_id(&conn, &run.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(restored, run);
        let research = restored.audit[0].research.as_ref().unwrap();
        assert_eq!(research.write_ups.len(), 1);
        assert_eq!(research.roster[0].published.as_deref(), Some("2025-03-27"));
        assert_eq!(research.roster[0].retrieved_at, "2026-09-16T12:00:00Z");
        assert_eq!(research.distillation.shape, crate::portfolio::distill::DistillationShape::Merged);
        assert_eq!(research.distillation.calls, 1);
        let analysis = restored.audit[0].analysis.as_ref().expect("the analysis record");
        assert_eq!(analysis.text, "The venue change of 2025-03-31 is the one dated fact.");
        assert_eq!(analysis.written, "2026-09-18");
        assert_eq!(analysis.anchor.as_ref().map(|a| a.date.as_str()), Some("2026-09-17"));
    }

    #[test]
    fn entry7_archive_round_trip_preserves_named_attempt_measurements() {
        use crate::local_model::{ApiCounters, PromptUsage, ThinkingObservation};
        let (_a, source) = temp_store();
        seed_store(&source);
        let conn = storage::open(&source.db_path).unwrap();
        let raw: String = conn
            .query_row(
                "SELECT run_json FROM portfolio_runs WHERE run_id = 'run-two'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let mut run: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let health = crate::portfolio::DataHealth {
            prompt_usage: vec![PromptUsage {
                stage: "holding-AAPL research t1 synthesis".into(),
                model: "qwen".into(),
                think: Some(true),
                format: true,
                prompt_chars: 1234,
                elapsed_ms: 5678,
                thinking: ThinkingObservation::Partial { chars: 9012 },
                api: ApiCounters {
                    eval_count: Some(3),
                    total_duration: Some(4),
                    load_duration: Some(5),
                    prompt_eval_count: Some(6),
                    prompt_eval_duration: Some(7),
                    eval_duration: Some(8),
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        run["roll_up"]["data_health"] = serde_json::to_value(&health).unwrap();
        conn.execute(
            "UPDATE portfolio_runs SET run_json = ?1 WHERE run_id = 'run-two'",
            [run.to_string()],
        )
        .unwrap();
        let dest = source.db_path.parent().unwrap().join("telemetry.zip");
        export_archive(&source, &dest, None).unwrap();
        let (_b, target) = temp_store();
        import_archive(&target, &dest, None, false).unwrap();
        let conn = storage::open(&target.db_path).unwrap();
        let raw: String = conn
            .query_row(
                "SELECT run_json FROM portfolio_runs WHERE run_id = 'run-two'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restored: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let restored: crate::portfolio::DataHealth =
            serde_json::from_value(restored["roll_up"]["data_health"].clone()).unwrap();
        assert_eq!(restored.prompt_usage, health.prompt_usage);
    }

    #[test]
    fn round_trip_preserves_rows_and_files_and_rederives_paths() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        let summary =
            export_archive(&source, &dest, None).unwrap();
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
        assert_eq!(loaded.portfolio_runs, 2);
        assert_eq!(loaded.holdings_pulls, 1);
        assert_eq!(loaded.skipped_reports, 0);

        // Full-row parity on every table; only the re-derived `markdown_path` and the
        // vector rows' local ids are excepted (the episode store's ids travel).
        for table in TABLES {
            let except: &[&str] = match table {
                "reports" => &["markdown_path"],
                "vector_memory" => &["id"],
                _ => &[],
            };
            let rows = table_rows(&source, table, except);
            assert!(!rows.is_empty(), "{table} is seeded");
            assert_eq!(rows, table_rows(&target, table, except), "{table}");
        }
        let conn = storage::open(&target.db_path).unwrap();
        let restored = crate::portfolio::store::run_by_id(&conn, "run-one")
            .unwrap()
            .unwrap();
        assert_eq!(
            restored,
            crate::portfolio::store::archive_test_run("run-one", "2026-07-06T12:00:00Z")
        );
        let markdown_path: String = conn
            .query_row(
                "SELECT markdown_path FROM reports WHERE report_id = 'report-one'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(markdown_path.starts_with(&target.reports_dir.to_string_lossy().into_owned()));
        assert_eq!(std::fs::read_to_string(&markdown_path).unwrap(), "# Issue one\n");
        let blob: Vec<u8> = conn
            .query_row(
                "SELECT embedding FROM vector_memory WHERE kind = 'summary'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(blob, embedding_to_blob(&[0.1, 0.2, 0.3]));
        assert_eq!(
            std::fs::read_to_string(target.archive_dir.join("filed-note.md")).unwrap(),
            "archived research\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.inbox_dir.join("pending-note.txt")).unwrap(),
            "inbox research\n"
        );
        assert_eq!(table_count(&target, "app_settings"), 0);
    }

    #[test]
    fn archive_never_contains_settings_or_source_paths() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

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
        assert_eq!(
            manifest.embedders,
            BTreeMap::from([(
                "report".to_string(),
                crate::embedding::EMBEDDING_MODEL.to_string()
            )])
        );
        // The db/*.ndjson entries ride the checksum inventory too.
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
        let summary = export_archive(&source, &dest, Some("hunter2")).unwrap();
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
        export_archive(&source, &dest, None).unwrap();

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
        std::fs::remove_file(
            source
                .reports_dir
                .join("2026-07-01-market-signal-report-report-o.md"),
        )
        .unwrap();
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        let (_b, target) = temp_store();
        let loaded = import_archive(&target, &dest, None, false).unwrap();
        assert_eq!(loaded.skipped_reports, 1);
        assert_eq!(loaded.reports, 1);
        let conn = storage::open(&target.db_path).unwrap();
        // report-one's summary row drops with it; report-two's snapshot is untouched.
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
        // report-two carries the seeded snapshot, so removing its body exercises the
        // snapshot leg of the cascade.
        std::fs::remove_file(
            source
                .reports_dir
                .join("2026-07-05-market-signal-report-report-t.md"),
        )
        .unwrap();
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        let (_b, target) = temp_store();
        let loaded = import_archive(&target, &dest, None, false).unwrap();
        assert_eq!(loaded.skipped_reports, 1);
        assert_eq!(loaded.reports, 1);
        assert_eq!(loaded.snapshots, 0);
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
        export_archive(&source, &dest, None).unwrap();

        // Re-stamped, so only the pre-destructive decode pass can catch it, not the
        // checksum verification.
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
        assert_eq!(table_count(&target, "reports"), 1);
        assert!(target.reports_dir.join("old-report.md").exists());
    }

    #[test]
    fn a_vector_row_outside_the_report_namespace_is_refused() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        let mut entries = read_archive_entries(&dest);
        let vectors_name = "db/vector_memory.ndjson";
        let tampered_rows: Vec<u8> = {
            let text = String::from_utf8(entries[vectors_name].clone()).unwrap();
            let mut out = Vec::new();
            for (i, line) in text.lines().enumerate() {
                let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
                if i == 0 {
                    v["namespace"] = "portfolio".into();
                }
                serde_json::to_writer(&mut out, &v).unwrap();
                out.push(b'\n');
            }
            out
        };
        replace_entry_rechecksummed(&mut entries, vectors_name, tampered_rows);
        let tampered_path = source.db_path.parent().unwrap().join("tampered.zip");
        rebuild_zip(&entries, &tampered_path);

        let (_b, target) = temp_store();
        seed_old_report(&target);
        let err = import_archive(&target, &tampered_path, None, true).unwrap_err();
        assert!(err.to_string().contains("\"portfolio\" namespace"), "{err}");
        assert_eq!(table_count(&target, "reports"), 1);
        assert!(target.reports_dir.join("old-report.md").exists());
    }

    #[test]
    fn an_entry_the_manifest_does_not_list_is_never_consumed() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();
        let (_b, target) = temp_store();

        // (a) An unlisted store file.
        let mut entries = read_archive_entries(&dest);
        entries.insert("reports/rogue.md".to_string(), b"# Rogue\n".to_vec());
        let rogue_path = source.db_path.parent().unwrap().join("rogue.zip");
        rebuild_zip(&entries, &rogue_path);
        let err = import_archive(&target, &rogue_path, None, false).unwrap_err();
        assert!(err.to_string().contains("not listed in the manifest"), "{err}");
        assert_eq!(table_count(&target, "reports"), 0);
        assert!(!target.reports_dir.join("rogue.md").exists());

        // (b) A db entry whose manifest listing was dropped.
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
        export_archive(&source, &dest, None).unwrap();

        // Re-stamped so only the uniqueness pre-check can catch it, not the checksum
        // verification; the INSERT would otherwise fail mid-transaction, after the
        // file mutations.
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
        export_archive(&source, &dest, None).unwrap();
        let info = inspect_archive(&dest, None).unwrap();
        assert!(!info.encrypted);
        assert_eq!(info.format_version, FORMAT_VERSION);
        assert_eq!(info.reports, 2);
        assert_eq!(info.learnings, 2);
        assert_eq!(info.snapshots, 1);
        assert_eq!(info.portfolio_runs, 2);
        assert_eq!(info.holdings_pulls, 1);
        assert_eq!(info.files, 4);
    }

    #[test]
    fn corrupted_file_entry_fails_its_checksum() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

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
        assert_eq!(table_count(&target, "reports"), 0);
    }

    #[test]
    fn a_newer_format_version_is_refused_by_inspect_and_import() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        // The manifest itself carries no checksum, so a plain rewrite suffices.
        let mut entries = read_archive_entries(&dest);
        let mut manifest: Manifest =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest.format_version = FORMAT_VERSION + 1;
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let newer_path = source.db_path.parent().unwrap().join("newer.zip");
        rebuild_zip(&entries, &newer_path);

        let err = inspect_archive(&newer_path, None).unwrap_err();
        assert!(err.to_string().contains("update Market Signal"), "{err}");
        let (_b, target) = temp_store();
        let err = import_archive(&target, &newer_path, None, false).unwrap_err();
        assert!(err.to_string().contains("update Market Signal"), "{err}");
        assert_eq!(table_count(&target, "reports"), 0);
    }

    #[test]
    fn an_archive_missing_a_table_entry_is_refused() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        // Both the entry and its listing dropped: without the required-entries check
        // this would import vector_memory as zero rows.
        let mut entries = read_archive_entries(&dest);
        entries.remove("db/vector_memory.ndjson");
        let mut manifest: Manifest =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest.files.retain(|f| f.path != "db/vector_memory.ndjson");
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let missing_path = source.db_path.parent().unwrap().join("missing.zip");
        rebuild_zip(&entries, &missing_path);

        let (_b, target) = temp_store();
        let err = import_archive(&target, &missing_path, None, false).unwrap_err();
        assert!(err.to_string().contains("missing or not listed"), "{err}");
        assert_eq!(table_count(&target, "vector_memory"), 0);
    }

    #[test]
    fn an_archive_missing_the_factor_store_entry_is_refused() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        // A current-format archive without the entry is truncated, never a store
        // with no observations.
        let mut entries = read_archive_entries(&dest);
        entries.remove("db/factor_observations.ndjson");
        let mut manifest: Manifest =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest.files.retain(|f| f.path != "db/factor_observations.ndjson");
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let missing_path = source.db_path.parent().unwrap().join("missing-factors.zip");
        rebuild_zip(&entries, &missing_path);

        let (_b, target) = temp_store();
        let err = import_archive(&target, &missing_path, None, false).unwrap_err();
        assert!(err.to_string().contains("db/factor_observations.ndjson"), "{err}");
        assert!(err.to_string().contains("missing or not listed"), "{err}");
        assert_eq!(table_count(&target, "reports"), 0, "nothing imported");
    }

    #[test]
    fn store_is_empty_counts_the_factor_store() {
        let (_a, paths) = temp_store();
        let conn = storage::open(&paths.db_path).unwrap();
        assert!(store_is_empty(&conn).unwrap());
        conn.execute(
            "INSERT INTO factor_observations (issuer, factor, sector, cap_band, value,
                                              floored, observed_on)
             VALUES ('AAPL', 'earnings-yield', 'Technology', 'mega', 0.031, 0, '2026-07-06')",
            [],
        )
        .unwrap();
        assert!(!store_is_empty(&conn).unwrap(), "an observation alone is analytical data");
    }

    #[test]
    fn a_v1_archive_without_the_quick_check_entry_imports_complete() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        // Under the archive's own version a missing quick-check entry is truncation…
        let mut entries = read_archive_entries(&dest);
        entries.remove("db/portfolio_quick_checks.ndjson");
        let mut manifest: Manifest =
            serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest
            .files
            .retain(|f| f.path != "db/portfolio_quick_checks.ndjson");
        manifest.row_counts.remove("portfolio_quick_checks");
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let truncated = source.db_path.parent().unwrap().join("truncated.zip");
        rebuild_zip(&entries, &truncated);
        let (_b, target) = temp_store();
        let err = import_archive(&target, &truncated, None, false).unwrap_err();
        assert!(err.to_string().contains("missing or not listed"), "{err}");

        // …while a v1 archive is complete at five entries: compatibility by version,
        // never sparse tolerance (`docs/data-portability.md` §Import flow).
        manifest.format_version = 1;
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let v1_path = source.db_path.parent().unwrap().join("v1.zip");
        rebuild_zip(&entries, &v1_path);
        let (_c, target) = temp_store();
        let loaded = import_archive(&target, &v1_path, None, false).unwrap();
        assert_eq!(loaded.reports, 2);
        assert_eq!(table_count(&target, "portfolio_quick_checks"), 0);
    }

    #[test]
    fn a_v1_archive_never_consumes_an_unlisted_later_entry() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        // A later format's entry present without a listing was never checksummed, so
        // it is refused rather than imported unverified.
        for unlisted in ["db/factor_observations.ndjson", "db/portfolio_quick_checks.ndjson"] {
            let mut entries = read_archive_entries(&dest);
            let mut manifest: Manifest =
                serde_json::from_slice(&entries["manifest.json"]).unwrap();
            manifest.format_version = 1;
            manifest.files.retain(|f| f.path != unlisted);
            entries.insert(
                "manifest.json".to_string(),
                serde_json::to_vec_pretty(&manifest).unwrap(),
            );
            let path = source.db_path.parent().unwrap().join("v1-unlisted.zip");
            rebuild_zip(&entries, &path);
            let (_t, target) = temp_store();
            let err = import_archive(&target, &path, None, false).unwrap_err();
            assert!(err.to_string().contains(unlisted), "{err}");
            assert!(err.to_string().contains("not listed in the manifest"), "{err}");
            assert_eq!(table_count(&target, "reports"), 0, "nothing imported");
            assert_eq!(table_count(&target, "factor_observations"), 0);
        }
    }

    #[test]
    fn a_v2_stamp_is_refused_as_a_pre_release_format() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        // Under the archive's own version the dropped entries are truncation…
        let mut entries = read_archive_entries(&dest);
        let dropped = [
            "db/portfolio_episodes.ndjson",
            "db/portfolio_episode_checks.ndjson",
            "db/price_bars.ndjson",
        ];
        for name in dropped {
            entries.remove(name);
        }
        let mut manifest: Manifest = serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest.files.retain(|f| !dropped.contains(&f.path.as_str()));
        manifest.row_counts.remove("portfolio_episodes");
        manifest.row_counts.remove("portfolio_episode_checks");
        manifest.row_counts.remove("price_bars");
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let truncated = source.db_path.parent().unwrap().join("truncated-v3.zip");
        rebuild_zip(&entries, &truncated);
        let (_b, target) = temp_store();
        let err = import_archive(&target, &truncated, None, false).unwrap_err();
        assert!(err.to_string().contains("missing or not listed"), "{err}");

        // …and re-stamping it v2 does not rescue it: a pre-release format is refused.
        manifest.format_version = 2;
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let v2_path = source.db_path.parent().unwrap().join("v2.zip");
        rebuild_zip(&entries, &v2_path);
        let (_c, target) = temp_store();
        let err = import_archive(&target, &v2_path, None, false).unwrap_err();
        assert!(err.to_string().contains("pre-release format"), "{err}");
        assert_eq!(table_count(&target, "reports"), 0, "nothing imported");
    }

    #[test]
    fn a_v3_stamp_is_refused_as_a_pre_release_format() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        // Under the archive's own version the dropped entries are truncation…
        let mut entries = read_archive_entries(&dest);
        for name in ["db/web_documents.ndjson", "db/web_source_state.ndjson"] {
            entries.remove(name);
        }
        let mut manifest: Manifest = serde_json::from_slice(&entries["manifest.json"]).unwrap();
        manifest.files.retain(|f| {
            f.path != "db/web_documents.ndjson" && f.path != "db/web_source_state.ndjson"
        });
        manifest.row_counts.remove("web_documents");
        manifest.row_counts.remove("web_source_state");
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let truncated = source.db_path.parent().unwrap().join("truncated-v7.zip");
        rebuild_zip(&entries, &truncated);
        let (_b, target) = temp_store();
        let err = import_archive(&target, &truncated, None, false).unwrap_err();
        assert!(err.to_string().contains("missing or not listed"), "{err}");

        // …and re-stamping it v3 does not rescue it: a pre-release format is refused.
        manifest.format_version = 3;
        entries.insert(
            "manifest.json".to_string(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        );
        let v3_path = source.db_path.parent().unwrap().join("v3.zip");
        rebuild_zip(&entries, &v3_path);
        let (_c, target) = temp_store();
        let err = import_archive(&target, &v3_path, None, false).unwrap_err();
        assert!(err.to_string().contains("pre-release format"), "{err}");
        assert_eq!(table_count(&target, "reports"), 0, "nothing imported");
    }

    #[test]
    fn every_pre_release_format_stamp_is_refused() {
        // A complete current export re-stamped v0 or any pre-release version is
        // refused outright, never read through a compat rung.
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();
        let mut entries = read_archive_entries(&dest);
        let mut manifest: Manifest = serde_json::from_slice(&entries["manifest.json"]).unwrap();
        for version in std::iter::once(0).chain(2..FORMAT_VERSION) {
            manifest.format_version = version;
            entries.insert(
                "manifest.json".to_string(),
                serde_json::to_vec_pretty(&manifest).unwrap(),
            );
            let path = source.db_path.parent().unwrap().join(format!("v{version}.zip"));
            rebuild_zip(&entries, &path);
            let (_b, target) = temp_store();
            let err = import_archive(&target, &path, None, false).unwrap_err();
            assert!(err.to_string().contains("pre-release format"), "v{version}: {err}");
            assert_eq!(table_count(&target, "reports"), 0, "nothing imported");
        }
    }

    #[test]
    fn duplicate_outcome_rows_are_refused_pre_destructively() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();

        // A duplicated episode id — the primary-key mirror.
        let mut entries = read_archive_entries(&dest);
        let text = String::from_utf8(entries["db/portfolio_episodes.ndjson"].clone()).unwrap();
        let doubled = format!("{text}{text}");
        replace_entry_rechecksummed(
            &mut entries,
            "db/portfolio_episodes.ndjson",
            doubled.into_bytes(),
        );
        let dup_path = source.db_path.parent().unwrap().join("dup-episode.zip");
        rebuild_zip(&entries, &dup_path);
        let (_b, target) = temp_store();
        let err = import_archive(&target, &dup_path, None, false).unwrap_err();
        assert!(err.to_string().contains("duplicate episode id"), "{err}");
        assert_eq!(table_count(&target, "portfolio_episodes"), 0);

        // A second check on the same episode and horizon — the UNIQUE mirror.
        let check_row = |id: i64, episode_id: i64| {
            format!(
                "{{\"id\":{id},\"episode_id\":{episode_id},\"horizon\":\"three_month\",\"run_id\":\"r\",\"check_json\":\"{{}}\"}}\n"
            )
        };
        let text =
            String::from_utf8(read_archive_entries(&dest)["db/portfolio_episode_checks.ndjson"].clone()).unwrap();
        for (tampered, needle) in [
            (format!("{text}{}", check_row(4, 7)), "two three_month checks for episode 7"),
            (check_row(4, 99), "for episode 99, which it does not carry"),
        ] {
            let mut entries = read_archive_entries(&dest);
            replace_entry_rechecksummed(
                &mut entries,
                "db/portfolio_episode_checks.ndjson",
                tampered.into_bytes(),
            );
            let path = source.db_path.parent().unwrap().join("bad-check.zip");
            rebuild_zip(&entries, &path);
            let (_t, target) = temp_store();
            let err = import_archive(&target, &path, None, false).unwrap_err();
            assert!(err.to_string().contains(needle), "{err}");
            assert_eq!(table_count(&target, "portfolio_episode_checks"), 0);
        }

        // A duplicated (symbol, date) bar — the primary-key mirror.
        let mut entries = read_archive_entries(&dest);
        let text = String::from_utf8(entries["db/price_bars.ndjson"].clone()).unwrap();
        let doubled = format!("{text}{text}");
        replace_entry_rechecksummed(&mut entries, "db/price_bars.ndjson", doubled.into_bytes());
        let dup_path = source.db_path.parent().unwrap().join("dup-bar.zip");
        rebuild_zip(&entries, &dup_path);
        let (_c, target) = temp_store();
        let err = import_archive(&target, &dup_path, None, false).unwrap_err();
        assert!(err.to_string().contains("duplicate price bar"), "{err}");
        assert_eq!(table_count(&target, "price_bars"), 0);
    }

    #[test]
    fn malformed_factor_observations_are_refused_pre_destructively() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();
        let text =
            String::from_utf8(read_archive_entries(&dest)["db/factor_observations.ndjson"].clone())
                .unwrap();
        let row = |value: &str, floored: bool| {
            format!(
                "{{\"issuer\":\"MSFT\",\"factor\":\"momentum\",\"sector\":\"Technology\",\"cap_band\":\"mega\",\"value\":{value},\"floored\":{floored},\"observed_on\":\"2026-07-06\"}}\n"
            )
        };
        for (tampered, needle) in [
            // A doubled (issuer, factor) key — the primary-key mirror.
            (format!("{text}{text}"), "duplicate debt-to-equity observation for AAPL"),
            // A value that is also floored, and neither — the check mirror.
            (format!("{text}{}", row("0.1", true)), "both or neither"),
            (format!("{text}{}", row("null", false)), "both or neither"),
            // An absent value key is refused, never read as floored.
            (
                format!("{text}{}", row("null", true).replace("\"value\":null,", "")),
                "missing field `value`",
            ),
        ] {
            let mut entries = read_archive_entries(&dest);
            replace_entry_rechecksummed(
                &mut entries,
                "db/factor_observations.ndjson",
                tampered.into_bytes(),
            );
            let path = source.db_path.parent().unwrap().join("bad-observation.zip");
            rebuild_zip(&entries, &path);
            let (_t, target) = temp_store();
            let err = import_archive(&target, &path, None, false).unwrap_err();
            assert!(format!("{err:#}").contains(needle), "{err:#}");
            assert_eq!(table_count(&target, "factor_observations"), 0);
            assert_eq!(table_count(&target, "reports"), 0, "nothing imported");
        }
    }

    #[test]
    fn traversal_paths_are_rejected_before_any_write() {
        let (_a, source) = temp_store();
        seed_store(&source);
        let dest = source.db_path.parent().unwrap().join("export.zip");
        export_archive(&source, &dest, None).unwrap();
        let (_b, target) = temp_store();

        // (a) A `..` component: the zip-slip guard rejects it at read time.
        let mut entries = read_archive_entries(&dest);
        add_entry_rechecksummed(&mut entries, "reports/../evil.md", b"# Evil\n".to_vec());
        let dotdot_path = source.db_path.parent().unwrap().join("dotdot.zip");
        rebuild_zip(&entries, &dotdot_path);
        let err = import_archive(&target, &dotdot_path, None, false).unwrap_err();
        assert!(err.to_string().contains("unsafe path"), "{err}");

        // (b) A nested path passes the zip-slip guard, so only the bare-name check
        // can catch it; manifest-listed so the checksum pass doesn't either.
        let mut entries = read_archive_entries(&dest);
        add_entry_rechecksummed(&mut entries, "reports/sub/evil.md", b"# Evil\n".to_vec());
        let nested_path = source.db_path.parent().unwrap().join("nested.zip");
        rebuild_zip(&entries, &nested_path);
        let err = import_archive(&target, &nested_path, None, false).unwrap_err();
        assert!(err.to_string().contains("unsafe path"), "{err}");
        assert!(!target.reports_dir.join("evil.md").exists());

        // (c) A report row whose markdown_filename escapes, re-checksummed.
        let mut entries = read_archive_entries(&dest);
        let text = String::from_utf8(entries["db/reports.ndjson"].clone()).unwrap();
        let mut out = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
            if i == 0 {
                v["markdown_filename"] = "../evil.md".into();
            }
            serde_json::to_writer(&mut out, &v).unwrap();
            out.push(b'\n');
        }
        replace_entry_rechecksummed(&mut entries, "db/reports.ndjson", out);
        let escape_path = source.db_path.parent().unwrap().join("escape.zip");
        rebuild_zip(&entries, &escape_path);
        let err = import_archive(&target, &escape_path, None, false).unwrap_err();
        assert!(err.to_string().contains("unsafe markdown filename"), "{err}");

        assert_eq!(table_count(&target, "reports"), 0);
    }

    #[test]
    fn an_archive_that_unpacks_past_the_ceiling_is_refused() {
        // Through the injectable limit; a multi-GB fixture would be absurd.
        let build = || {
            let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
            let opts =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file("db/reports.ndjson", opts).unwrap();
            zip.write_all(&[b'x'; 4096]).unwrap();
            zip.finish().unwrap().into_inner()
        };
        let mut archive = ZipArchive::new(Cursor::new(build())).unwrap();
        let err = read_entries_bounded(&mut archive, 1024).unwrap_err();
        assert!(err.to_string().contains("ceiling"), "{err}");

        let mut archive = ZipArchive::new(Cursor::new(build())).unwrap();
        let entries = read_entries_bounded(&mut archive, 8192).unwrap();
        assert_eq!(entries["db/reports.ndjson"].len(), 4096);
    }

    #[test]
    fn an_oversized_manifest_is_refused() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let opts =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("manifest.json", opts).unwrap();
        zip.write_all(&vec![b' '; 4096]).unwrap();
        let bytes = zip.finish().unwrap().into_inner();

        let mut archive = ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
        let err = read_manifest_bounded(&mut archive, 1024).unwrap_err();
        assert!(err.to_string().contains("ceiling"), "{err}");

        // Under a roomy ceiling the same entry reaches the JSON parser: the bound
        // fires first, the parser second.
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let err = read_manifest_bounded(&mut archive, 8192).unwrap_err();
        assert!(err.to_string().contains("parsing manifest.json"), "{err}");
    }

    #[test]
    fn key_derivation_parameters_are_frozen() {
        // Pins the derivation bit-for-bit. If this test fails, do NOT update the
        // expected value: existing encrypted archives could no longer be opened.
        // Keep this derivation for `MSDPENC1` and introduce a new container version.
        let key = derive_key("hunter2", b"0123456789abcdef").unwrap();
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "9d4c62adf54ad88fef393e9fda5cb6c12823c19d68ab04cb0a6adca43ca389c0"
        );
    }
}
