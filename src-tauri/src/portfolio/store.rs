//! SQLite persistence for Portfolio Analysis runs (`docs/storage.md §Local Analysis
//! Suite Storage`): free functions over `&rusqlite::Connection`, the tables
//! created by [`init_schema`] (called from `storage::init_schema`). A run is
//! persisted whole as one JSON blob plus the columns the UI lists on.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::portfolio::{PortfolioRun, PORTFOLIO_RUN_RETENTION};
use crate::schwab::Holdings;

/// Idempotent. A new constraint on an exported table needs a matching import
/// pre-check in `portability::import_archive`.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_runs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id      TEXT NOT NULL UNIQUE,
            created_at  TEXT NOT NULL,
            run_json    TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS holdings_pulls (
            id            INTEGER PRIMARY KEY CHECK (id = 1),
            pulled_at     TEXT NOT NULL,
            holdings_json TEXT NOT NULL
        )",
        [],
    )?;
    // A single latest row, deliberately separate from `portfolio_runs`: a quick
    // check must never surface in the run history or become the next full run's
    // diff baseline (`docs/portfolio-analysis.md §The quick check`).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_quick_checks (
            id         INTEGER PRIMARY KEY CHECK (id = 1),
            checked_at TEXT NOT NULL,
            state_json TEXT NOT NULL
        )",
        [],
    )?;
    // The episode store (`docs/portfolio-analysis.md §Outcome learning`):
    // insert-only, and persisted independent of the run retention, since a
    // three-year horizon outlives any count-based window. The autoincrement ids
    // are insertion order — an episode's names a symbol's latest, a check's is
    // run order — and `UNIQUE (episode_id, horizon)` holds a horizon to one check.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_episodes (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            symbol       TEXT NOT NULL,
            created_on   TEXT NOT NULL,
            episode_json TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_episode_checks (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            episode_id INTEGER NOT NULL,
            horizon    TEXT NOT NULL,
            run_id     TEXT NOT NULL,
            check_json TEXT NOT NULL,
            UNIQUE (episode_id, horizon)
        )",
        [],
    )?;
    // The shared price-bar cache (`docs/storage.md §Local Analysis Suite
    // Storage`): split-adjusted daily closes keyed by symbol.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS price_bars (
            symbol TEXT NOT NULL,
            date   TEXT NOT NULL,
            close  REAL NOT NULL,
            PRIMARY KEY (symbol, date)
        )",
        [],
    )?;
    // The interrupted-run checkpoint store (`docs/portfolio-analysis.md §Failure
    // posture`): one header row plus one row per completed holding. At most one
    // interrupted run exists; a new run discards the lot as it opens its own
    // trail, and a successful persist clears its own. Deliberately excluded from
    // the data-portability archive: transient machine-local state that expires
    // at the resume window, so no import pre-check mirrors it.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_checkpoints (
            run_id            TEXT PRIMARY KEY,
            created_at        TEXT NOT NULL,
            header_json       TEXT NOT NULL,
            accumulators_json TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_checkpoint_holdings (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id   TEXT NOT NULL,
            symbol   TEXT NOT NULL,
            row_json TEXT NOT NULL,
            UNIQUE (run_id, symbol)
        )",
        [],
    )?;
    Ok(())
}

// ---- Interrupted-run checkpoints (`docs/portfolio-analysis.md §Failure posture`) --

/// The pinned header a resume reopens (`docs/portfolio-analysis.md §Failure
/// posture`), so completed and resumed holdings alike compute against one book
/// and one run-level context. Per-holding retrieval is deliberately not pinned:
/// it runs live at each holding's own analysis (`docs/portfolio-workflow.md`
/// §Step 2), so a resumed holding's evidence is fresher than the pinned as-of by
/// up to the resume window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointHeader {
    pub run_id: String,
    /// The pinned as-of the resume window ages against.
    pub created_at: String,
    /// The diff / carry baseline at run start. Resume refuses when the store's
    /// latest run has moved — any run that persisted discarded prior checkpoints
    /// on its way, so a mismatch means stale state.
    pub prior_run_id: Option<String>,
    pub holdings: Holdings,
    pub rates: crate::portfolio::engine::RateAnchors,
    pub house_view: crate::portfolio::dossier::HouseView,
    pub house_view_omitted: bool,
    pub commodities: crate::portfolio::dossier::CommodityContext,
    pub cot_rows: Vec<crate::data_sources::CotPositioning>,
    pub cot_gaps: Vec<String>,
    pub put_call_backdrop: Option<crate::cboe::PutCallBackdrop>,
    pub cboe_gap: Option<String>,
    pub short_interest_file: Option<crate::finra::ShortInterestFile>,
    pub finra_gap: Option<String>,
    /// The run-level M&A feed each held stock matches against at dossier
    /// assembly, and the walk's gap where it failed or truncated.
    pub ma_deals: Vec<crate::portfolio::evidence::MaDeal>,
    pub ma_gap: Option<String>,
    /// A selective run's work-list (uppercased); `None` = the whole book.
    pub work_list: Option<Vec<String>>,
    pub swept_tail: Vec<crate::portfolio::quick_check::HoldingQuickState>,
    /// Version stamps at run start — a resume refuses on any mismatch with the
    /// current binary / roster rather than mixing verdicts across contracts.
    pub prompt_version: String,
    pub grade_parameter_version: String,
    pub target_parameter_version: String,
    pub pre_profit_parameter_version: String,
    /// The semantics of the selective carried-tail sweep pinned in this header.
    pub quick_check_parameter_version: String,
    /// The rule the trail's completed holdings were floored under
    /// ([`crate::portfolio::engine::EVIDENCE_FLOOR_VERSION`]).
    pub evidence_floor_version: String,
    /// The trail's own shape stamp ([`CHECKPOINT_FORMAT_VERSION`]), so a trail
    /// under another header or row shape is refused at the resume gate with its
    /// reason rather than loud-skipping every row.
    pub checkpoint_format_version: String,
    pub model_ids: Vec<String>,
}

/// The checkpoint trail's shape and pinned-input stamp — what the header and
/// each holding row persist. It moves whenever a change alters the trail's
/// shape or the semantics of a pinned input restored by resume, and only then:
/// what a completed holding's verdict, audit and carried tail *mean* is stamped
/// on the six version axes the header carries (`docs/portfolio-analysis.md`
/// §Failure posture).
pub const CHECKPOINT_FORMAT_VERSION: &str = "checkpoint-v26";

/// The run-level keyed identities the post-loop consumers read, re-written
/// beside each holding checkpoint so completed holdings' contributions survive
/// a failure. Every entry is keyed by symbol, so a re-analysis overwrites its
/// own entry and a resume can never count a holding twice. Nothing counted
/// lives here: counts ride each [`CheckpointHolding`], so the trail's
/// contribution membership is its row membership by construction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckpointAccumulators {
    pub sector_by_symbol:
        std::collections::HashMap<String, crate::portfolio::sector::SectorIdentity>,
    pub industry_by_symbol: std::collections::HashMap<String, Option<String>>,
    pub profile_name_by_symbol: std::collections::HashMap<String, Option<String>>,
}

/// One completed holding's contribution to the run-level data-health counts,
/// carried on its checkpoint row so a resume rebuilds the counts from the rows
/// it restored rather than seeding a cumulative counter — one re-written whole
/// beside the next holding's write survived a dropped row and counted that
/// holding again when it re-analyzed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HoldingHealth {
    /// The deep-history (FMP dated-EOD) fetch degraded, starving the anchor
    /// window to its fallback.
    pub deep_history_failed: bool,
    /// The sector-benchmark series this holding read as unavailable, off a
    /// fresh failed fetch or the per-process memo alike; the run-level list
    /// rebuilt from the rows deduplicates by benchmark. `None` where no
    /// benchmark was read or the series resolved.
    pub benchmark_gap: Option<String>,
}

/// One completed holding's checkpoint: what the loop pushed for it, plus its
/// own local-model telemetry and its data-health contribution. Riding the row
/// rather than the accumulators makes the trail's telemetry membership its row
/// membership by construction — a row that never landed or no longer reads
/// takes its calls with it, and that holding re-analyzes whole
/// (`docs/portfolio-analysis.md` §Failure posture). No `serde(default)` on any
/// field: a row under another shape takes the loud-skip at load and
/// re-analyzes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckpointHolding {
    pub verdict: crate::portfolio::HoldingVerdict,
    pub audit: crate::portfolio::HoldingAudit,
    /// This holding's rows of the data-health context-fit read
    /// (`build_data_health`).
    pub prompt_usage: Vec<crate::local_model::PromptUsage>,
    pub model_retries: Vec<crate::local_model::RetryEvent>,
    pub health: HoldingHealth,
}

#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub header: CheckpointHeader,
    pub accumulators: CheckpointAccumulators,
    pub holdings: Vec<CheckpointHolding>,
}

/// Opens the trail for a starting run with fresh accumulators.
pub fn save_checkpoint_header(conn: &Connection, header: &CheckpointHeader) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO portfolio_checkpoints
         (run_id, created_at, header_json, accumulators_json)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            header.run_id,
            header.created_at,
            serde_json::to_string(header)?,
            serde_json::to_string(&CheckpointAccumulators::default())?,
        ],
    )?;
    Ok(())
}

/// Idempotent per symbol: a resumed run re-checkpointing a symbol replaces its
/// row. The row and the refreshed accumulators land in one transaction, since a
/// crash between the two writes could otherwise restore the verdict without its
/// accumulator contribution, or the contribution without its row.
pub fn save_checkpoint_progress(
    conn: &Connection,
    run_id: &str,
    symbol: &str,
    row: &CheckpointHolding,
    acc: &CheckpointAccumulators,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO portfolio_checkpoint_holdings (run_id, symbol, row_json)
         VALUES (?1, ?2, ?3)",
        params![run_id, symbol.to_ascii_uppercase(), serde_json::to_string(row)?],
    )?;
    tx.execute(
        "UPDATE portfolio_checkpoints SET accumulators_json = ?2 WHERE run_id = ?1",
        params![run_id, serde_json::to_string(acc)?],
    )?;
    tx.commit()?;
    Ok(())
}

/// An unreadable header or row is loud-skipped: a corrupt checkpoint costs the
/// resume offer, never an error surface. A header stamped under another trail
/// format loads alone — its accumulators and rows are not read, since the gate
/// refuses the trail on the stamp — so a stale trail costs its refusal reason,
/// never a loud-skip per row.
pub fn load_checkpoint(conn: &Connection) -> Result<Option<Checkpoint>> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT run_id, header_json, accumulators_json FROM portfolio_checkpoints
             ORDER BY created_at DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((run_id, header_json, acc_json)) = row else {
        return Ok(None);
    };
    let header: CheckpointHeader = match serde_json::from_str(&header_json) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("portfolio checkpoint: unreadable header for {run_id} ({e}) — skipped");
            return Ok(None);
        }
    };
    if header.checkpoint_format_version != CHECKPOINT_FORMAT_VERSION {
        return Ok(Some(Checkpoint {
            header,
            accumulators: CheckpointAccumulators::default(),
            holdings: Vec::new(),
        }));
    }
    let accumulators: CheckpointAccumulators = match serde_json::from_str(&acc_json) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "portfolio checkpoint: unreadable accumulators for {run_id} ({e}) — skipped"
            );
            return Ok(None);
        }
    };
    let mut stmt = conn.prepare(
        "SELECT symbol, row_json FROM portfolio_checkpoint_holdings
         WHERE run_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![run_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut holdings = Vec::new();
    for row in rows {
        let (symbol, json) = row?;
        match serde_json::from_str::<CheckpointHolding>(&json) {
            Ok(h) => holdings.push(h),
            Err(e) => eprintln!(
                "portfolio checkpoint: unreadable holding row {symbol} ({e}) — it will re-analyze"
            ),
        }
    }
    Ok(Some(Checkpoint {
        header,
        accumulators,
        holdings,
    }))
}

/// A new run's entry path and a completed resume's cleanup
/// (`docs/portfolio-analysis.md §Failure posture`).
pub fn clear_checkpoints(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM portfolio_checkpoint_holdings", [])?;
    conn.execute("DELETE FROM portfolio_checkpoints", [])?;
    Ok(())
}

// ---- The episode store (`docs/portfolio-analysis.md §Outcome learning`) ---------

/// Every readable episode and check in insertion order, every `(episode,
/// horizon)` a check row exists for — off the SQL columns, so an unreadable
/// check still holds its horizon written and is never re-due — and the count
/// of rows that no longer decoded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EpisodeStore {
    pub episodes: Vec<crate::portfolio::outcome::StoredEpisode>,
    pub checks: Vec<crate::portfolio::outcome::StoredCheck>,
    pub written: std::collections::HashSet<(i64, crate::portfolio::outcome::Horizon)>,
    pub skipped: usize,
}

/// Oldest first by insertion order (`id`) — the order that names a symbol's
/// latest episode and a check's run order; the creation date is data, never
/// identity. A row whose JSON no longer decodes is skipped and logged — never a
/// load failure and never deleted: an unreadable episode is never due, and its
/// checks count in no score.
pub fn load_episode_store(conn: &Connection) -> Result<EpisodeStore> {
    use crate::portfolio::outcome::{StoredCheck, StoredEpisode};
    let mut out = EpisodeStore::default();
    let mut stmt =
        conn.prepare("SELECT id, episode_json FROM portfolio_episodes ORDER BY id ASC")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, json) = row?;
        match serde_json::from_str(&json) {
            Ok(record) => out.episodes.push(StoredEpisode { id, record }),
            Err(e) => {
                eprintln!("outcome learning: skipping unreadable episode row {id}: {e}");
                out.skipped += 1;
            }
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id, episode_id, horizon, check_json FROM portfolio_episode_checks ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (id, episode_id, horizon, json) = row?;
        if let Some(h) = crate::portfolio::outcome::Horizon::from_key(&horizon) {
            out.written.insert((episode_id, h));
        }
        match serde_json::from_str(&json) {
            Ok(check) => out.checks.push(StoredCheck { id, episode_id, check }),
            Err(e) => {
                eprintln!("outcome learning: skipping unreadable check row {id}: {e}");
                out.skipped += 1;
            }
        }
    }
    Ok(out)
}

/// The store's only episode write; an episode is never updated or deleted.
pub fn insert_episode(
    conn: &Connection,
    record: &crate::portfolio::outcome::PriceRecord,
) -> Result<crate::portfolio::outcome::StoredEpisode> {
    conn.execute(
        "INSERT INTO portfolio_episodes (symbol, created_on, episode_json) VALUES (?1, ?2, ?3)",
        params![
            record.symbol.to_ascii_uppercase(),
            record.created_on,
            serde_json::to_string(record)?
        ],
    )?;
    Ok(crate::portfolio::outcome::StoredEpisode {
        id: conn.last_insert_rowid(),
        record: record.clone(),
    })
}

/// A second check for the same episode and horizon is refused by the schema,
/// so a horizon can never be re-scored.
pub fn insert_check(
    conn: &Connection,
    episode_id: i64,
    check: &crate::portfolio::outcome::Check,
) -> Result<crate::portfolio::outcome::StoredCheck> {
    conn.execute(
        "INSERT INTO portfolio_episode_checks (episode_id, horizon, run_id, check_json)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            episode_id,
            check.horizon.key(),
            check.run_id,
            serde_json::to_string(check)?
        ],
    )?;
    Ok(crate::portfolio::outcome::StoredCheck {
        id: conn.last_insert_rowid(),
        episode_id,
        check: check.clone(),
    })
}

/// Each symbol's latest episode creation date, latest by insertion order — the
/// opening cadence's read. Off the SQL columns alone, so a row whose JSON no
/// longer decodes still holds its symbol's cadence.
pub fn latest_episode_dates(conn: &Connection) -> Result<std::collections::HashMap<String, String>> {
    let mut stmt =
        conn.prepare("SELECT symbol, created_on FROM portfolio_episodes ORDER BY id ASC")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut out = std::collections::HashMap::new();
    for row in rows {
        let (symbol, created_on) = row?;
        out.insert(symbol, created_on);
    }
    Ok(out)
}

// ---- Price-bar cache (`docs/storage.md §Local Analysis Suite Storage`) ----------

/// A symbol's cached daily closes, oldest first. Symbols are stored uppercase.
pub fn load_price_bars(
    conn: &Connection,
    symbol: &str,
) -> Result<Vec<crate::portfolio::engine::DatedValue>> {
    let mut stmt =
        conn.prepare("SELECT date, close FROM price_bars WHERE symbol = ?1 ORDER BY date ASC")?;
    let rows = stmt.query_map([symbol.to_ascii_uppercase()], |row| {
        Ok(crate::portfolio::engine::DatedValue {
            date: row.get(0)?,
            value: row.get(1)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Merge freshly fetched bars into a symbol's cache — a shared date takes the
/// fresh close (a split-adjusted refetch re-bases history, so newer wins).
pub fn merge_price_bars(
    conn: &Connection,
    symbol: &str,
    bars: &[crate::portfolio::engine::DatedValue],
) -> Result<()> {
    let key = symbol.to_ascii_uppercase();
    let mut stmt = conn
        .prepare("INSERT OR REPLACE INTO price_bars (symbol, date, close) VALUES (?1, ?2, ?3)")?;
    for bar in bars {
        stmt.execute(params![key, bar.date, bar.value])?;
    }
    Ok(())
}

/// Latest-only, like `holdings_pulls`.
pub fn save_quick_check(
    conn: &Connection,
    state: &crate::portfolio::quick_check::QuickCheckState,
) -> Result<()> {
    let state_json = serde_json::to_string(state)?;
    conn.execute(
        "INSERT INTO portfolio_quick_checks (id, checked_at, state_json)
         VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET
             checked_at = excluded.checked_at,
             state_json = excluded.state_json",
        params![state.last_checked_at, state_json],
    )?;
    Ok(())
}

/// `None` before any quick check ran or after a full pass cleared it. Decode is
/// loud-skip, like [`latest_run`]: an unparseable state logs one warning and
/// reads as `None`, never an `Err`, so a corrupt row neither blanks the
/// Portfolio page load nor silently drops the quick-check chaining at the job.
/// Only a store read error is an `Err`.
pub fn latest_quick_check(
    conn: &Connection,
) -> Result<Option<crate::portfolio::quick_check::QuickCheckState>> {
    let json = conn
        .query_row(
            "SELECT state_json FROM portfolio_quick_checks WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(json.and_then(|j| match serde_json::from_str(&j) {
        Ok(state) => Some(state),
        Err(e) => {
            eprintln!("[portfolio-store] skipping unparseable quick-check state: {e}");
            None
        }
    }))
}

/// The successful full pass's clear (`docs/portfolio-analysis.md §The quick
/// check`).
pub fn clear_quick_check(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM portfolio_quick_checks", [])?;
    Ok(())
}

/// The latest standalone Pull holdings snapshot (`docs/portfolio-analysis.md
/// §Triggering`, `docs/storage.md §Local Analysis Suite Storage`): view-only,
/// never read by the job — the run's own snapshot is the holdings-diff baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingsPull {
    /// Canonical UTC RFC3339; the frontend renders local time.
    pub pulled_at: String,
    pub holdings: Holdings,
}

/// Latest-only: replaces any prior snapshot.
pub fn save_pull(conn: &Connection, pull: &HoldingsPull) -> Result<()> {
    let holdings_json = serde_json::to_string(&pull.holdings)?;
    conn.execute(
        "INSERT INTO holdings_pulls (id, pulled_at, holdings_json)
         VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET
             pulled_at = excluded.pulled_at,
             holdings_json = excluded.holdings_json",
        params![pull.pulled_at, holdings_json],
    )?;
    Ok(())
}

/// `None` before any pull.
pub fn latest_pull(conn: &Connection) -> Result<Option<HoldingsPull>> {
    let row = conn
        .query_row(
            "SELECT pulled_at, holdings_json FROM holdings_pulls WHERE id = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    match row {
        Some((pulled_at, json)) => Ok(Some(HoldingsPull {
            pulled_at,
            holdings: serde_json::from_str(&json)?,
        })),
        None => Ok(None),
    }
}

/// The unique `run_id` makes a re-insert of the same run a clean error rather
/// than a silent duplicate. The write validates before it lands: the serialized
/// record is decoded back through its own type, and a record that would not
/// read back — a non-finite required float serde wrote as `null` — is refused
/// naming the holding, so the run fails under its hard persistence posture
/// (`docs/portfolio-analysis.md` §Failure posture) instead of persisting a row
/// every later read would loud-skip. The producers' finite-or-absent guards
/// make this unreachable from the engine; the seam closes the class for
/// whatever they miss.
pub fn insert_run(conn: &Connection, run: &PortfolioRun) -> Result<()> {
    let run_json = serde_json::to_string(run)?;
    if let Err(e) = serde_json::from_str::<PortfolioRun>(&run_json) {
        let holding = unreadable_holding(run)
            .map(|symbol| format!(" — holding {symbol}"))
            .unwrap_or_default();
        anyhow::bail!(
            "portfolio run {} would not read back from its own record{holding}: {e}",
            run.run_id
        );
    }
    conn.execute(
        "INSERT INTO portfolio_runs (run_id, created_at, run_json)
         VALUES (?1, ?2, ?3)",
        params![run.run_id, run.created_at, run_json],
    )?;
    Ok(())
}

/// The first holding whose audit or verdict does not survive its own JSON
/// round-trip; `None` when the unreadable value sits outside the per-holding
/// records.
fn unreadable_holding(run: &PortfolioRun) -> Option<String> {
    fn survives<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> bool {
        serde_json::to_string(value)
            .ok()
            .is_some_and(|json| serde_json::from_str::<T>(&json).is_ok())
    }
    run.audit
        .iter()
        .find(|a| !survives(*a))
        .map(|a| a.symbol.clone())
        .or_else(|| {
            run.verdicts
                .iter()
                .find(|v| !survives(*v))
                .map(|v| v.symbol.clone())
        })
}

/// `None` before any run exists (`docs/portfolio-analysis.md` §Continuity and
/// isolation). Newest-first by insertion order (`id`), never `created_at`: the
/// identity of "latest" is the run most recently persisted — under a backwards
/// clock step a `created_at` ordering would hand the diff / carry baseline back
/// to the prior run while the just-persisted one sat invisible. Decode is
/// loud-skip: an unparseable blob logs one warning and the walk continues to
/// the next row, since the job caller fails soft to "no prior run" and an `Err`
/// here would silently become no diff baseline and no carries on the strength
/// of one corrupt row. Only a store read error is an `Err`.
pub fn latest_run(conn: &Connection) -> Result<Option<PortfolioRun>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, run_json FROM portfolio_runs
         ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (run_id, json) = row?;
        if let Some(run) = decode_run(&run_id, &json) {
            return Ok(Some(run));
        }
    }
    Ok(None)
}

/// Loud-skip on failure, single-homed here: unparseable blobs are a
/// deliberately retained store state, so one must never blank the runs history
/// or the diff baseline for its retention life.
fn decode_run(run_id: &str, json: &str) -> Option<PortfolioRun> {
    match serde_json::from_str::<PortfolioRun>(json) {
        Ok(run) => Some(run),
        Err(e) => {
            eprintln!("[portfolio-store] skipping unparseable run {run_id}: {e}");
            None
        }
    }
}

/// Newest first, capped at `limit`; the same insertion-order identity and
/// loud-skip as [`latest_run`].
pub fn list_recent_runs(conn: &Connection, limit: u32) -> Result<Vec<PortfolioRun>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, run_json FROM portfolio_runs
         ORDER BY id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (run_id, json) = row?;
        if let Some(run) = decode_run(&run_id, &json) {
            out.push(run);
        }
    }
    Ok(out)
}

/// One sidebar row of the run history (`docs/interface.md §Main Layout`):
/// never the run's verdict payload, so the listing response stays rows, not
/// full runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortfolioRunSummary {
    pub run_id: String,
    /// Canonical UTC RFC3339; the frontend renders local time.
    pub created_at: String,
    pub holdings_count: usize,
    pub graded_count: usize,
    /// Whether the run's blob decoded. An unreadable row still lists — its
    /// identity is column-backed — so the history shows the row exists instead
    /// of silently shrinking, and the page can tell an unreadable store from a
    /// never-ran one. The counts are zero on an unreadable row.
    pub readable: bool,
}

/// Newest first, capped at `limit`; the same ordering as [`latest_run`] /
/// [`prune_runs`], so the list shows exactly the retained window. An unreadable
/// blob still yields a row with `readable: false`.
pub fn list_run_summaries(conn: &Connection, limit: u32) -> Result<Vec<PortfolioRunSummary>> {
    let mut stmt = conn.prepare(
        "SELECT run_id, created_at, run_json FROM portfolio_runs
         ORDER BY id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (run_id, created_at, json) = row?;
        out.push(match decode_run(&run_id, &json) {
            Some(run) => PortfolioRunSummary {
                holdings_count: run.holdings.positions.len(),
                graded_count: run.roll_up.graded_count,
                run_id: run.run_id,
                created_at: run.created_at,
                readable: true,
            },
            None => PortfolioRunSummary {
                run_id,
                created_at,
                holdings_count: 0,
                graded_count: 0,
                readable: false,
            },
        });
    }
    Ok(out)
}

/// `None` when the id is unknown (e.g. pruned between listing and click — the
/// frontend re-lists rather than erroring).
pub fn run_by_id(conn: &Connection, run_id: &str) -> Result<Option<PortfolioRun>> {
    let json = conn
        .query_row(
            "SELECT run_json FROM portfolio_runs WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    // A corrupt row reads as not-found. The summaries still list it, rendered
    // disabled, so only a stale or out-of-band caller reaches here, where the
    // frontend's re-list recovery is right.
    Ok(json.and_then(|j| decode_run(run_id, &j)))
}

/// [`latest_run`] returns `None` both for a never-ran store and for one whose
/// only rows are unreadable; this cheap column read lets a surface tell the two
/// apart and refuse honestly.
pub fn any_runs(conn: &Connection) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM portfolio_runs)",
        [],
        |r| r.get(0),
    )?;
    Ok(exists)
}

/// The per-feature retention cascade (`docs/storage.md §Local Analysis Suite
/// Storage`); degraded runs count against the one cap. Idempotent.
pub fn prune_runs(conn: &Connection, keep: u32) -> Result<()> {
    // Insertion-order retention, matching [`latest_run`] / [`list_recent_runs`],
    // so eviction removes exactly the runs the history no longer shows and the
    // run being persisted (the max id) is inside the keep set even under a
    // backwards wall-clock step — a `created_at` ordering could evict it inside
    // its own transaction, leaving a Successful job pointing at a phantom run_id.
    conn.execute(
        "DELETE FROM portfolio_runs
         WHERE id NOT IN (
             SELECT id FROM portfolio_runs
             ORDER BY id DESC
             LIMIT ?1
         )",
        [keep],
    )?;
    Ok(())
}

/// The job's one persist call: insert, then prune to the retention cap.
pub fn record_run(conn: &Connection, run: &PortfolioRun) -> Result<()> {
    insert_run(conn, run)?;
    prune_runs(conn, PORTFOLIO_RUN_RETENTION)?;
    Ok(())
}

/// The round-trip fixture the store and portability tests share: a run whose
/// first audit carries a populated research record.
#[cfg(test)]
pub(crate) fn entry3_test_run() -> PortfolioRun {
    use crate::portfolio::research::{PageRosterEntry, ResearchAuditRecord, TopicWriteUp};
    let mut run = tests::sample_run("entry3", "2026-09-19T00:00:00Z");
    run.audit[0].research = Some(ResearchAuditRecord {
        write_ups: vec![TopicWriteUp {
            topic_key: "exposure-profile".into(),
            title: "Exposure profile".into(),
            write_up: Some("Reconstructed dating evidence: the venue change took effect on 2025-03-31 (https://example.com/reconstructed-arkf).".into()),
            passes: 1,
            skipped: None,
        }],
        disconfirming: Some("Nothing found against the exposure read.".into()),
        roster: vec![PageRosterEntry {
            url: "https://example.com/reconstructed-arkf".into(),
            title: "ARKF venue change".into(),
            published: Some("2025-03-27".into()),
            retrieved_at: "2026-09-16T12:00:00Z".into(),
            source_tier: Some(1),
        }],
        fetches_spent: 2,
        elapsed_secs: 0,
        gaps: vec![],
        distillation: crate::portfolio::distill::DistillationRecord {
            shape: crate::portfolio::distill::DistillationShape::Merged,
            calls: 1,
        },
    });
    run.audit[0].analysis = Some(crate::portfolio::AnalysisRecord {
        text: "The venue change of 2025-03-31 is the one dated fact.".into(),
        written: "2026-09-18".into(),
        anchor: Some(crate::portfolio::engine::DatedValue { date: "2026-09-17".into(), value: 48.2 }),
    });
    run
}

/// The episode-store fixture the archive round trip and [`archive_test_run`]
/// share.
#[cfg(test)]
pub(crate) fn archive_test_episode() -> (
    crate::portfolio::outcome::PriceRecord,
    crate::portfolio::outcome::Check,
) {
    use crate::portfolio::engine::DatedValue;
    use crate::portfolio::outcome::*;
    let record = PriceRecord {
        symbol: "AAPL".into(),
        created_on: "2026-04-01".into(),
        spot: 180.0,
        anchor: Some(DatedValue { date: "2026-03-31".into(), value: 179.5 }),
        model: HorizonPrices {
            three_month: Some(190.0),
            twelve_month: Some(205.0),
            three_year: None,
        },
        engine: HorizonPrices {
            three_month: Some(186.0),
            twelve_month: None,
            three_year: Some(240.0),
        },
    };
    let check = Check {
        horizon: Horizon::ThreeMonth,
        checked_on: "2026-07-06".into(),
        run_id: "run-one".into(),
        outcome: CheckOutcome::Scored {
            close: DatedValue { date: "2026-07-01".into(), value: 195.25 },
            bridge_factor: 1.0,
            model: Some(LegScore { expected: 190.0, score: 97.31 }),
            engine: Some(LegScore { expected: 186.0, score: 95.26 }),
        },
    };
    (record, check)
}

/// The archive round trip's run: [`entry3_test_run`] with every optional
/// record set, so the round trip exercises them all.
#[cfg(test)]
pub(crate) fn archive_test_run(run_id: &str, created_at: &str) -> PortfolioRun {
    use crate::portfolio::outcome::*;
    let mut run = entry3_test_run();
    run.run_id = run_id.into();
    run.created_at = created_at.into();
    run.verdicts[0].position_change = crate::portfolio::PositionChange::Increased;
    run.verdicts[0].basis_move = Some(crate::portfolio::BasisMove {
        direction: crate::portfolio::BasisDirection::PaidUp,
        prior_average_cost: 140.0,
        average_cost: 150.0,
    });
    let audit = &mut run.audit[0];
    audit.review = Some("The prior read held: the twelve-month price is on its path.".into());
    audit.accuracy_read_through = Some(3);
    audit.fund_exposure = Some(crate::portfolio::fund::FundExposureBasis {
        class_label: "US equity fund".into(),
        expense_ratio: Some(0.0009),
        us_share: Some(0.98),
        top_sector: Some(("Technology".into(), 0.31)),
        structural_flag: false,
        nav: Some(512.34),
    });
    audit.pre_profit = Some(crate::portfolio::pre_profit::compute_overlay(
        &crate::portfolio::engine::CompanyFinancials::default(),
    ));
    let (record, check) = archive_test_episode();
    let arm = |score: f64| ArmScore {
        score,
        checks: 1,
        last_moved_on: "2026-07-06".into(),
        last_moved_check: 3,
    };
    run.accuracy = AccuracyRecord {
        scores: Some(std::collections::BTreeMap::from([(
            "AAPL".to_string(),
            AccuracyScores {
                three_month: HorizonAccuracy {
                    model: Some(arm(97.31)),
                    engine: Some(arm(95.26)),
                },
                ..Default::default()
            },
        )])),
        opened: vec![StoredEpisode { id: 7, record }],
        checks: vec![StoredCheck { id: 3, episode_id: 7, check }],
    };
    run
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{
        engine::ComputedMetrics, AssetClass, HoldingAudit, HoldingVerdict, PortfolioRollUp,
        PositionChange, VerdictDisposition,
    };
    use crate::schwab::{Holdings, Position};

    #[test]
    fn the_header_pins_the_ma_feed_and_a_v22_header_without_it_does_not_read() {
        let conn = mem();
        let run = sample_run("ma-feed", "2026-10-08T12:00:00Z");
        let mut header = checkpoint_header(&run);
        header.ma_deals = vec![crate::portfolio::evidence::MaDeal {
            acquirer_symbol: Some("AAPL".into()),
            target_symbol: Some("MDV".into()),
            transaction_date: "2026-06-01".into(),
            ..Default::default()
        }];
        header.ma_gap = Some("FMP M&A feed truncated at page 3 (rate-limited)".into());
        save_checkpoint_header(&conn, &header).unwrap();
        let cp = load_checkpoint(&conn).unwrap().expect("the header loads");
        assert_eq!(cp.header.ma_deals, header.ma_deals);
        assert_eq!(cp.header.ma_gap, header.ma_gap);
        // Required and always written (the pre-release posture), never defaulted
        // to an empty feed.
        let mut json = serde_json::to_value(&header).unwrap();
        let obj = json.as_object_mut().unwrap();
        obj.remove("ma_deals");
        obj.remove("ma_gap");
        assert!(serde_json::from_value::<CheckpointHeader>(json).is_err());
    }

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::init_schema(&conn).unwrap();
        conn
    }

    /// A current-format checkpoint header pinned to `run`.
    fn checkpoint_header(run: &PortfolioRun) -> CheckpointHeader {
        CheckpointHeader {
            run_id: run.run_id.clone(),
            created_at: run.created_at.clone(),
            prior_run_id: None,
            holdings: run.holdings.clone(),
            rates: Default::default(),
            house_view: Default::default(),
            house_view_omitted: false,
            commodities: Default::default(),
            cot_rows: vec![],
            cot_gaps: vec![],
            put_call_backdrop: None,
            cboe_gap: None,
            short_interest_file: None,
            finra_gap: None,
            ma_deals: vec![],
            ma_gap: None,
            work_list: None,
            swept_tail: vec![],
            prompt_version: crate::portfolio::PROMPT_VERSION.into(),
            grade_parameter_version: crate::portfolio::engine::GRADE_PARAMETER_VERSION.into(),
            target_parameter_version:
                crate::portfolio::engine::SCENARIO_TARGET_PARAMETER_VERSION.into(),
            pre_profit_parameter_version:
                crate::portfolio::pre_profit::PRE_PROFIT_PARAMETER_VERSION.into(),
            quick_check_parameter_version:
                crate::portfolio::quick_check::QUICK_CHECK_PARAMETER_VERSION.into(),
            evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.into(),
            checkpoint_format_version: CHECKPOINT_FORMAT_VERSION.into(),
            model_ids: vec!["stub-analyst".into()],
        }
    }

    #[test]
    fn entry7_finished_run_preserves_all_observations_and_rejects_missing_list() {
        use crate::local_model::{ApiCounters, PromptUsage, ThinkingObservation};
        let conn = mem();
        let mut run = sample_run("telemetry", "2026-09-19T12:00:00Z");
        run.roll_up.data_health.prompt_usage = vec![
            PromptUsage {
                stage: "ordinary".into(),
                model: "qwen".into(),
                adapter_ok: true,
                prompt_chars: 123,
                elapsed_ms: 456,
                thinking: ThinkingObservation::Complete { chars: 789 },
                api: ApiCounters {
                    eval_count: Some(2),
                    total_duration: Some(1000),
                    ..Default::default()
                },
                ..Default::default()
            },
            PromptUsage {
                stage: "failed".into(),
                elapsed_ms: 99,
                thinking: ThinkingObservation::Partial { chars: 3 },
                ..Default::default()
            },
            PromptUsage {
                stage: "length without counters".into(),
                output_limited: true,
                done_reason: Some("length".into()),
                ..Default::default()
            },
        ];
        insert_run(&conn, &run).unwrap();
        clear_checkpoints(&conn).unwrap();
        assert_eq!(
            latest_run(&conn)
                .unwrap()
                .unwrap()
                .roll_up
                .data_health
                .prompt_usage,
            run.roll_up.data_health.prompt_usage
        );
        let mut retired = serde_json::to_value(&run).unwrap();
        retired["roll_up"]["data_health"]
            .as_object_mut()
            .unwrap()
            .remove("prompt_usage");
        assert!(serde_json::from_value::<PortfolioRun>(retired).is_err());
        let mut retired_row =
            serde_json::to_value(&run.roll_up.data_health.prompt_usage[0]).unwrap();
        retired_row.as_object_mut().unwrap().remove("thinking");
        assert!(serde_json::from_value::<PromptUsage>(retired_row).is_err());
    }

    #[test]
    fn checkpoint_rows_round_trip_telemetry_health_and_the_format_stamp() {
        let conn = mem();
        let run = sample_run("run-1", "2026-08-28T12:00:00+00:00");
        let header = checkpoint_header(&run);
        save_checkpoint_header(&conn, &header).unwrap();
        let acc = CheckpointAccumulators {
            sector_by_symbol: [(
                "AAPL".to_string(),
                crate::portfolio::sector::SectorIdentity::resolve(Some("Technology")),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let row = CheckpointHolding {
            verdict: run.verdicts[0].clone(),
            audit: run.audit[0].clone(),
            prompt_usage: vec![crate::local_model::PromptUsage {
                stage: "interpret AAPL".into(),
                model: "qwen".into(),
                think: Some(true),
                format: true,
                streamed: true,
                adapter_ok: true,
                elapsed_ms: 321,
                thinking: crate::local_model::ThinkingObservation::Complete { chars: 456 },
                num_ctx: 131_072,
                prompt_chars: 480_000,
                num_predict: Some(8_192),
                output_limited: false,
                api: crate::local_model::ApiCounters {
                    prompt_eval_count: Some(120_000),
                    eval_count: Some(2_000),
                    ..Default::default()
                },
                ..Default::default()
            }],
            model_retries: vec![crate::local_model::RetryEvent {
                stage: "interpret AAPL".into(),
                cause: "transport-level connection failure".into(),
            }],
            health: HoldingHealth {
                deep_history_failed: true,
                benchmark_gap: Some("XLK".into()),
            },
        };
        save_checkpoint_progress(&conn, &run.run_id, "AAPL", &row, &acc).unwrap();
        let cp = load_checkpoint(&conn).unwrap().expect("trail exists");
        assert_eq!(cp.accumulators, acc);
        assert_eq!(cp.holdings, vec![row]);
        assert_eq!(cp.header.checkpoint_format_version, CHECKPOINT_FORMAT_VERSION);
    }

    #[test]
    fn a_trail_under_another_format_loads_its_header_alone() {
        let conn = mem();
        let run = sample_run("run-1", "2026-08-28T12:00:00+00:00");
        let mut header = checkpoint_header(&run);
        header.checkpoint_format_version = "checkpoint-v12".into();
        save_checkpoint_header(&conn, &header).unwrap();
        let acc = CheckpointAccumulators {
            sector_by_symbol: [(
                "AAPL".to_string(),
                crate::portfolio::sector::SectorIdentity::resolve(Some("Technology")),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let row = CheckpointHolding {
            verdict: run.verdicts[0].clone(),
            audit: run.audit[0].clone(),
            prompt_usage: vec![],
            model_retries: vec![],
            health: HoldingHealth::default(),
        };
        save_checkpoint_progress(&conn, &run.run_id, "AAPL", &row, &acc).unwrap();
        let cp = load_checkpoint(&conn).unwrap().expect("the header loads");
        assert_eq!(cp.header.checkpoint_format_version, "checkpoint-v12");
        assert!(
            cp.holdings.is_empty(),
            "rows under another format are not read"
        );
        assert_eq!(cp.accumulators, CheckpointAccumulators::default());
    }

    pub(super) fn sample_run(run_id: &str, created_at: &str) -> PortfolioRun {
        let position = Position {
            symbol: "AAPL".into(),
            description: "Apple".into(),
            asset_class: AssetClass::Stock,
            quantity: 100.0,
            cost_basis: 14_000.0,
            market_value: 19_500.0,
            current_price: Some(195.0),
        };
        PortfolioRun {
            run_id: run_id.into(),
            created_at: created_at.into(),
            holdings: Holdings {
                positions: vec![position],
                cash: 10_000.0,
                account_total: 29_500.0,
                source_rows: vec![],
            },
            verdicts: vec![HoldingVerdict {
                symbol: "AAPL".into(),
                asset_class: AssetClass::Stock,
                position_change: PositionChange::New,
                basis_move: None,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                analyzed_at: None,
                action_source: Default::default(),
                side_reversed: false,
            }],
            roll_up: PortfolioRollUp {
                role_risk_only_count: 0,
                failed_count: 0,
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                top_position_weight: 0.66,
                cash_weight: 0.34,
                exited: vec![],
                data_health: Default::default(),
                overview: "single fixture holding".into(),
            },
            audit: vec![HoldingAudit {
                research: None,
                analysis: None,
                review: None,
                accuracy_read_through: None,
                target_meta: None,
                symbol: "AAPL".into(),
                metrics: ComputedMetrics::default(),
                sources: vec!["FMP".into()],
                model_ids: vec!["qwen3.5:122b".into()],
                prompt_version: "portfolio-v1".into(),
                evidence_floor_version: crate::portfolio::engine::EVIDENCE_FLOOR_VERSION.to_string(),
                degraded_inputs: vec![],
                action_annotations: vec![],
                grade_parameter_version: crate::portfolio::engine::GRADE_PARAMETER_VERSION.to_string(),
                factors: None,
                quick_basis: None,
                authoring_close: None,
                fund_exposure: None,
                pre_profit: None,
                hurdle: None,
                forensic: None,
                soft_forensic: None,
                tech_event_pre_flag: None,
                short_interest: None,
                implied_expectations: None,
                narrative: None,
                option_overlay: None,
            }],
            rate_prints: Default::default(),
            failed_holdings: Vec::new(),
            accuracy: Default::default(),
        }
    }

    #[test]
    fn entry3_populated_research_round_trips_and_legacy_shapes_are_loud_skips() {
        let conn = mem();
        let run = entry3_test_run();
        insert_run(&conn, &run).unwrap();
        assert_eq!(latest_run(&conn).unwrap().unwrap(), run);
        let mut header = checkpoint_header(&run);
        save_checkpoint_header(&conn, &header).unwrap();
        let row = CheckpointHolding {
            verdict: run.verdicts[0].clone(),
            audit: run.audit[0].clone(),
            prompt_usage: vec![],
            model_retries: vec![],
            health: HoldingHealth::default(),
        };
        save_checkpoint_progress(
            &conn,
            &run.run_id,
            "AAPL",
            &row,
            &CheckpointAccumulators::default(),
        )
        .unwrap();
        assert_eq!(load_checkpoint(&conn).unwrap().unwrap().holdings, vec![row]);

        // A retired research shape is a loud skip, never a defaulted read (the
        // pre-release posture).
        let mut legacy_run = serde_json::to_value(&run).unwrap();
        let research = legacy_run["audit"][0]["research"].as_object_mut().unwrap();
        research.remove("write_ups");
        research.remove("roster");
        research.insert("combined".into(), serde_json::json!("Reconstructed dating evidence"));
        research.insert("seed_layer".into(), serde_json::json!([]));
        conn.execute(
            "UPDATE portfolio_runs SET run_json=?1",
            [legacy_run.to_string()],
        )
        .unwrap();
        assert!(latest_run(&conn).unwrap().is_none());
        // A record with no distillation shape is the same loud skip: the shape is
        // required, never defaulted to `none`.
        let mut v21_run = serde_json::to_value(&run).unwrap();
        let research = v21_run["audit"][0]["research"].as_object_mut().unwrap();
        assert!(research.remove("distillation").is_some());
        conn.execute(
            "UPDATE portfolio_runs SET run_json=?1",
            [v21_run.to_string()],
        )
        .unwrap();
        assert!(latest_run(&conn).unwrap().is_none());
        // Every retired header is refused before decoding deliberately corrupt rows.
        conn.execute(
            "UPDATE portfolio_checkpoint_holdings SET row_json='invalid'",
            [],
        )
        .unwrap();
        for version in 1..=14 {
            header.checkpoint_format_version = format!("checkpoint-v{version}");
            save_checkpoint_header(&conn, &header).unwrap();
            let checkpoint = load_checkpoint(&conn).unwrap().unwrap();
            assert_eq!(checkpoint.header.checkpoint_format_version, header.checkpoint_format_version);
            assert!(checkpoint.holdings.is_empty(), "v{version}");
        }
    }

    #[test]
    fn run_round_trips_through_storage() {
        let conn = mem();
        let run = sample_run("run-1", "2026-06-25T12:00:00Z");
        insert_run(&conn, &run).unwrap();
        let back = latest_run(&conn).unwrap().unwrap();
        assert_eq!(back, run, "the whole run round-trips");
    }

    /// An explicit `null` (a fund's) decodes as none; only an absent key is the
    /// skip.
    #[test]
    fn a_run_row_missing_the_composite_or_the_grade_basis_key_is_skipped() {
        use crate::portfolio::pipeline::{self, tests as pt, StubAnalyst};
        let stock = pt::dossier(AssetClass::Stock, pt::strong_financials());
        let (priced, audit) =
            pipeline::analyze_holding(&StubAnalyst, &stock, &pt::rates(), "2026-08-03").unwrap();
        let mut run = sample_run("run-1", "2026-06-25T12:00:00Z");
        run.verdicts[0] = priced;
        run.audit[0] = audit;
        let json = serde_json::to_value(&run).unwrap();
        let back = decode_run("run-1", &json.to_string()).expect("the written run reads back");
        assert!(back.audit[0].factors.is_some());
        let VerdictDisposition::Priced(g) = &back.verdicts[0].disposition else { panic!("priced") };
        assert!(g.grade_basis.is_some());

        fn parent<'a>(run: &'a mut serde_json::Value, key: &str) -> &'a mut serde_json::Value {
            match key {
                "factors" => &mut run["audit"][0],
                _ => &mut run["verdicts"][0]["disposition"],
            }
        }
        for key in ["factors", "grade_basis"] {
            let mut missing = json.clone();
            parent(&mut missing, key).as_object_mut().unwrap().remove(key).expect("the key was written");
            assert_eq!(decode_run("run-1", &missing.to_string()), None, "{key}");
            let mut null = json.clone();
            parent(&mut null, key)[key] = serde_json::Value::Null;
            assert!(decode_run("run-1", &null.to_string()).is_some(), "{key}: null");
        }
    }

    #[test]
    fn priced_and_role_risk_verdicts_round_trip_with_their_thesis_documents() {
        use crate::portfolio::pipeline::{self, tests as pt, StubAnalyst};
        let conn = mem();
        let stock = pt::dossier(AssetClass::Stock, pt::strong_financials());
        let (mut priced, audit) =
            pipeline::analyze_holding(&StubAnalyst, &stock, &pt::rates(), "2026-08-03").unwrap();
        let VerdictDisposition::Priced(g) = &mut priced.disposition else { panic!("priced") };
        assert!(!g.thesis_document.is_empty() && g.appendix.conviction.is_some());
        g.appendix.expected_price_3y = None;
        let role_risk = HoldingVerdict {
            symbol: "BND".into(),
            asset_class: AssetClass::Etf,
            position_change: PositionChange::New,
            basis_move: None,
            disposition: VerdictDisposition::RoleRiskOnly(Box::new(
                pipeline::role_risk_verdict_from_model_arm(
                    &crate::portfolio::fund::RoleRiskReadout {
                        class_label: "bond fund".into(),
                        ..Default::default()
                    },
                    "Role: a bond sleeve.\n\nRisks: duration.".into(),
                ),
            )),
            analyzed_at: Some("2026-08-03T20:00:00Z".into()),
            action_source: Default::default(),
            side_reversed: false,
        };
        let abstained = HoldingVerdict {
            symbol: "XYZ".into(),
            asset_class: AssetClass::Stock,
            position_change: PositionChange::Unchanged,
            basis_move: None,
            disposition: VerdictDisposition::InsufficientEvidence {
                reason: "inconclusive re-read".into(),
                prior_thesis_document: Some("The prior document.".into()),
            },
            analyzed_at: Some("2026-07-20T20:00:00Z".into()),
            action_source: Default::default(),
            side_reversed: false,
        };
        let mut run = sample_run("run-verdicts", "2026-08-03T20:00:00Z");
        run.verdicts = vec![priced, role_risk, abstained];
        run.audit = vec![audit];
        insert_run(&conn, &run).unwrap();
        let back = latest_run(&conn).unwrap().unwrap();
        assert_eq!(back, run, "every disposition round-trips");
        let VerdictDisposition::Priced(g) = &back.verdicts[0].disposition else { panic!("priced") };
        assert_eq!(g.appendix.expected_price_3y, None, "a null expected price stays null");
        assert_eq!(back.verdicts[1].thesis_document(), Some("Role: a bond sleeve.\n\nRisks: duration."));
        assert_eq!(back.verdicts[2].thesis_document(), Some("The prior document."));
    }

    #[test]
    fn serde_writes_a_non_finite_float_as_null_and_refuses_null_into_a_bare_f64() {
        // Pinned rather than assumed: every JSON-number pass-through in the
        // record is finite by the parser, so only the app's own arithmetic can
        // produce a non-finite value.
        assert_eq!(serde_json::to_string(&f64::INFINITY).unwrap(), "null");
        assert_eq!(serde_json::to_string(&f64::NAN).unwrap(), "null");
        assert!(serde_json::from_str::<f64>("null").is_err());
        assert_eq!(serde_json::from_str::<Option<f64>>("null").unwrap(), None);
        assert!(
            serde_json::from_str::<f64>("1e999").is_err(),
            "an out-of-range literal is rejected at parse, never read as inf"
        );
    }

    #[test]
    fn a_run_that_would_not_read_back_is_refused_at_the_write_naming_the_holding() {
        let conn = mem();
        record_run(&conn, &sample_run("run-good", "2026-08-10T00:00:00Z")).unwrap();
        let mut poisoned = sample_run("run-poisoned", "2026-08-11T00:00:00Z");
        poisoned.audit[0].quick_basis = Some(crate::portfolio::engine::QuickCheckBasis {
            spot: 195.0,
            drivers: [6.0, 7.0, 8.0],
            spread_percentiles: None,
            raw_percentiles: None,
            forward_dividends: f64::INFINITY,
            dispersion_floor: 0.05,
            consensus_eps_mid: None,
            consensus_eps_periods: Vec::new(),
        });
        let err = insert_run(&conn, &poisoned).unwrap_err().to_string();
        assert!(
            err.contains("would not read back") && err.contains("holding AAPL"),
            "{err}"
        );
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM portfolio_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "no unreadable row landed");
        assert_eq!(latest_run(&conn).unwrap().unwrap().run_id, "run-good");
    }

    #[test]
    fn a_run_built_over_feed_extreme_inputs_round_trips_through_the_store() {
        // Finite extremes through the real producers: every derivation reads
        // absent where its arithmetic did not finish, nothing persists
        // non-finite, and the run reads back equal.
        use crate::portfolio::engine::{
            self, AnchorObservation, CompanyFinancials, DatedValue, QuickCheckBasis,
        };
        let conn = mem();
        let fin = CompanyFinancials {
            revenue: Some(f64::MAX),
            revenue_prior: Some(1e-300),
            total_debt: Some(f64::MAX),
            total_equity: Some(1e-300),
            // What an unguarded shaper sum would have handed over.
            ttm_dividends_per_share: Some(f64::INFINITY),
            price_history: vec![1e-310, 1.0, 1.0, 1.0],
            ..Default::default()
        };
        let metrics = engine::compute_metrics(&fin);
        let forward_dividends = engine::finite(fin.ttm_dividends_per_share).unwrap_or(0.0);
        let floor = engine::dispersion_floor(metrics.return_volatility);
        let observations = [AnchorObservation {
            spread: None,
            raw_multiple: 1e-300,
        }];
        let set = engine::spread_anchored_scenarios(
            1e10,
            [1.0, 1.0, 1.0],
            &observations,
            0.04,
            forward_dividends,
            floor,
        );
        let dv = |date: &str, value: f64| DatedValue {
            date: date.to_string(),
            value,
        };
        let closes = vec![dv("2026-01-02", 1e-310), dv("2026-01-05", 100.0)];
        let bench = vec![dv("2026-01-02", 1.0), dv("2026-01-05", 1.0)];

        let mut run = sample_run("run-extreme", "2026-08-12T00:00:00Z");
        let audit = &mut run.audit[0];
        audit.metrics = metrics;
        audit.quick_basis = Some(QuickCheckBasis {
            spot: 1e10,
            drivers: [1.0, 1.0, 1.0],
            spread_percentiles: set.spread_percentiles,
            raw_percentiles: set.raw_percentiles,
            forward_dividends,
            dispersion_floor: floor,
            consensus_eps_mid: None,
            consensus_eps_periods: Vec::new(),
        });
        audit.implied_expectations =
            engine::implied_expectations(1e10, &set, Some(1.0), "eps", true, 0.04);
        audit.narrative =
            engine::narrative_vs_reality(&fin, 1e300, Some(1e-300), &[], Some(30)).ok();
        audit.tech_event_pre_flag =
            engine::tech_event_pre_flag(&closes, &bench, "XLK", "2026-01-02", Some(0.02)).ok();

        insert_run(&conn, &run).unwrap();
        let back = latest_run(&conn).unwrap().unwrap();
        assert_eq!(back, run, "the extreme-input run round-trips");
        let a = &back.audit[0];
        assert_eq!(a.metrics.revenue_growth, None, "{:?}", a.metrics);
        assert_eq!(a.metrics.return_volatility, None, "{:?}", a.metrics);
        assert_eq!(a.quick_basis.as_ref().unwrap().forward_dividends, 0.0);
        assert_eq!(a.implied_expectations, None);
        assert_eq!(a.narrative, None);
        assert_eq!(a.tech_event_pre_flag, None);
    }

    #[test]
    fn a_pre_profit_overlay_round_trips_through_the_run() {
        let overlay = crate::portfolio::pre_profit::compute_overlay(
            &crate::portfolio::engine::CompanyFinancials::default(),
        );
        let conn = mem();
        let mut run = sample_run("run-1", "2026-06-25T12:00:00Z");
        run.audit[0].pre_profit = Some(overlay);
        insert_run(&conn, &run).unwrap();
        let back = latest_run(&conn).unwrap().unwrap();
        assert_eq!(back, run, "the run with its overlay round-trips");
    }

    #[test]
    fn quick_check_basis_and_rate_prints_round_trip() {
        let conn = mem();
        let mut run = sample_run("run-1", "2026-08-03T12:00:00Z");
        run.rate_prints = crate::portfolio::RatePrints {
            dgs2: 0.04,
            dgs10: 0.045,
            dgs2_as_of: Some("2026-08-01".into()),
            dgs10_as_of: Some("2026-08-01".into()),
            fetched_at: "2026-08-03T12:00:00Z".into(),
        };
        run.audit[0].quick_basis = Some(crate::portfolio::engine::QuickCheckBasis {
            spot: 195.0,
            drivers: [6.0, 6.5, 7.0],
            spread_percentiles: Some([0.002, 0.001, 0.0005]),
            raw_percentiles: Some([25.0, 28.0, 31.0]),
            forward_dividends: 1.0,
            dispersion_floor: 0.05,
            consensus_eps_mid: Some(6.5),
            consensus_eps_periods: vec![crate::portfolio::engine::ConsensusEpsPeriod {
                period_end: "2026-12-31".into(),
                eps_mid: Some(6.5),
                ntm_weight: 1.0,
            }],
        });
        run.audit[0].fund_exposure = Some(crate::portfolio::fund::FundExposureBasis {
            class_label: "US equity fund".into(),
            expense_ratio: Some(0.0009),
            us_share: Some(0.97),
            top_sector: Some(("Technology".into(), 0.31)),
            structural_flag: false,
            nav: None,
        });
        insert_run(&conn, &run).unwrap();
        assert_eq!(latest_run(&conn).unwrap().unwrap(), run);
    }

    #[test]
    fn pre_v25_quick_basis_decodes_without_period_rows() {
        let basis: crate::portfolio::engine::QuickCheckBasis = serde_json::from_value(
            serde_json::json!({
                "spot": 195.0,
                "drivers": [6.0, 6.5, 7.0],
                "spread_percentiles": null,
                "raw_percentiles": [25.0, 28.0, 31.0],
                "forward_dividends": 1.0,
                "dispersion_floor": 0.05,
                "consensus_eps_mid": 6.5
            }),
        )
        .unwrap();
        assert_eq!(basis.consensus_eps_mid, Some(6.5));
        assert!(basis.consensus_eps_periods.is_empty());
    }


    #[test]
    fn latest_run_is_none_before_any_insert() {
        assert!(latest_run(&mem()).unwrap().is_none());
    }

    #[test]
    fn record_run_enforces_retention_keeping_the_newest_n() {
        let conn = mem();
        // One more than the cap, ascending timestamps.
        for i in 0..(PORTFOLIO_RUN_RETENTION + 1) {
            let created_at = format!("2026-06-{:02}T00:00:00Z", i + 1);
            record_run(&conn, &sample_run(&format!("run-{i:02}"), &created_at)).unwrap();
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM portfolio_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, PORTFOLIO_RUN_RETENTION as i64, "pruned to the cap");
        let surviving: Vec<String> = list_recent_runs(&conn, 100)
            .unwrap()
            .into_iter()
            .map(|r| r.run_id)
            .collect();
        assert!(!surviving.contains(&"run-00".to_string()));
        assert_eq!(
            latest_run(&conn).unwrap().unwrap().run_id,
            format!("run-{PORTFOLIO_RUN_RETENTION:02}")
        );
    }

    #[test]
    fn a_backdated_run_keeps_full_production_identity_under_insertion_order() {
        // A backwards wall-clock step must not demote the just-persisted run;
        // insertion order is the production identity everywhere.
        let conn = mem();
        for i in 0..PORTFOLIO_RUN_RETENTION {
            let created_at = format!("2026-06-{:02}T00:00:00Z", i + 2);
            record_run(&conn, &sample_run(&format!("run-{i:02}"), &created_at)).unwrap();
        }
        // The clock steps back before every retained run.
        record_run(&conn, &sample_run("run-backdated", "2026-06-01T00:00:00Z")).unwrap();
        assert_eq!(latest_run(&conn).unwrap().unwrap().run_id, "run-backdated");
        let capped: Vec<String> = list_recent_runs(&conn, PORTFOLIO_RUN_RETENTION)
            .unwrap()
            .into_iter()
            .map(|r| r.run_id)
            .collect();
        assert_eq!(capped.first().map(String::as_str), Some("run-backdated"));
        assert_eq!(capped.len() as u32, PORTFOLIO_RUN_RETENTION);
        assert!(!capped.contains(&"run-00".to_string()), "the oldest-inserted evicted");
    }

    #[test]
    fn run_summaries_list_newest_first_and_run_by_id_round_trips() {
        let conn = mem();
        record_run(&conn, &sample_run("run-a", "2026-07-01T00:00:00Z")).unwrap();
        record_run(&conn, &sample_run("run-b", "2026-07-02T00:00:00Z")).unwrap();
        let summaries = list_run_summaries(&conn, 10).unwrap();
        assert_eq!(
            summaries,
            vec![
                PortfolioRunSummary {
                    run_id: "run-b".into(),
                    created_at: "2026-07-02T00:00:00Z".into(),
                    holdings_count: 1,
                    graded_count: 0,
                    readable: true,
                },
                PortfolioRunSummary {
                    run_id: "run-a".into(),
                    created_at: "2026-07-01T00:00:00Z".into(),
                    holdings_count: 1,
                    graded_count: 0,
                    readable: true,
                },
            ],
            "newest first, light rows only"
        );
        assert_eq!(list_run_summaries(&conn, 1).unwrap().len(), 1);
        let back = run_by_id(&conn, "run-a").unwrap().unwrap();
        assert_eq!(back.run_id, "run-a");
        assert!(run_by_id(&conn, "run-gone").unwrap().is_none());
    }

    #[test]
    fn latest_run_skips_an_unparseable_blob_instead_of_erroring() {
        // The callers fail-soft with `.ok().flatten()`, so an `Err` here would
        // silently become "no prior run" on the strength of one corrupt row.
        let conn = mem();
        record_run(&conn, &sample_run("run-good", "2026-08-10T00:00:00Z")).unwrap();
        conn.execute(
            "INSERT INTO portfolio_runs (run_id, created_at, run_json) \
             VALUES ('run-corrupt', '2026-08-11T00:00:00Z', '{\"not\": \"a run\"}')",
            [],
        )
        .unwrap();
        let latest = latest_run(&conn).unwrap().expect("the older readable run is served");
        assert_eq!(latest.run_id, "run-good");
    }


    #[test]
    fn latest_quick_check_skips_an_unparseable_state_instead_of_erroring() {
        let conn = mem();
        conn.execute(
            "INSERT INTO portfolio_quick_checks (id, checked_at, state_json) \
             VALUES (1, '2026-08-11T00:00:00Z', '{\"not\": \"a state\"}')",
            [],
        )
        .unwrap();
        assert!(latest_quick_check(&conn).unwrap().is_none());
        let state = crate::portfolio::quick_check::QuickCheckState {
            parameter_version:
                crate::portfolio::quick_check::QUICK_CHECK_PARAMETER_VERSION.into(),
            swept_run_id: "run-1".into(),
            last_checked_at: "2026-08-12T00:00:00Z".into(),
            rate_cache: None,
            holdings: vec![],
        };
        save_quick_check(&conn, &state).unwrap();
        assert_eq!(latest_quick_check(&conn).unwrap().unwrap(), state);
    }

    #[test]
    fn a_corrupt_row_costs_its_own_listing_row_never_the_history() {
        let conn = mem();
        record_run(&conn, &sample_run("run-good", "2026-08-10T00:00:00Z")).unwrap();
        conn.execute(
            "INSERT INTO portfolio_runs (run_id, created_at, run_json) \
             VALUES ('run-corrupt', '2026-08-11T00:00:00Z', 'not json')",
            [],
        )
        .unwrap();
        let ids: Vec<String> = list_recent_runs(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|r| r.run_id)
            .collect();
        assert_eq!(ids, vec!["run-good".to_string()], "the corrupt row skips loudly");
        let summaries = list_run_summaries(&conn, 10).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].run_id, "run-corrupt");
        assert!(!summaries[0].readable);
        assert_eq!(summaries[0].holdings_count, 0);
        assert!(summaries[1].readable);
        assert!(run_by_id(&conn, "run-corrupt").unwrap().is_none());
    }

    #[test]
    fn any_runs_splits_never_ran_from_all_unreadable() {
        let conn = mem();
        assert!(!any_runs(&conn).unwrap());
        conn.execute(
            "INSERT INTO portfolio_runs (run_id, created_at, run_json) \
             VALUES ('run-corrupt', '2026-08-11T00:00:00Z', 'not json')",
            [],
        )
        .unwrap();
        assert!(any_runs(&conn).unwrap(), "a persisted (if unreadable) row is a run");
        assert!(latest_run(&conn).unwrap().is_none(), "but not a decodable baseline");
    }

    #[test]
    fn duplicate_run_id_is_rejected() {
        let conn = mem();
        let run = sample_run("dup", "2026-06-25T12:00:00Z");
        insert_run(&conn, &run).unwrap();
        assert!(insert_run(&conn, &run).is_err(), "run_id is unique");
    }

    fn sample_pull(pulled_at: &str, quantity: f64) -> HoldingsPull {
        HoldingsPull {
            pulled_at: pulled_at.into(),
            holdings: Holdings {
                positions: vec![Position {
                    symbol: "AAPL".into(),
                    description: "Apple".into(),
                    asset_class: AssetClass::Stock,
                    quantity,
                    cost_basis: 14_000.0,
                    market_value: 19_500.0,
                    current_price: Some(195.0),
                }],
                cash: 10_000.0,
                account_total: 29_500.0,
                source_rows: vec![],
            },
        }
    }

    #[test]
    fn pull_round_trips_and_is_none_before_any_save() {
        let conn = mem();
        assert!(latest_pull(&conn).unwrap().is_none());
        let pull = sample_pull("2026-07-07T12:00:00Z", 100.0);
        save_pull(&conn, &pull).unwrap();
        assert_eq!(latest_pull(&conn).unwrap().unwrap(), pull);
    }

    #[test]
    fn save_pull_replaces_the_prior_snapshot() {
        let conn = mem();
        save_pull(&conn, &sample_pull("2026-07-07T12:00:00Z", 100.0)).unwrap();
        save_pull(&conn, &sample_pull("2026-07-07T15:00:00Z", 150.0)).unwrap();
        let back = latest_pull(&conn).unwrap().unwrap();
        assert_eq!(back.pulled_at, "2026-07-07T15:00:00Z");
        assert_eq!(back.holdings.positions[0].quantity, 150.0);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM holdings_pulls", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "latest-only: a single row, replaced in place");
    }

    #[test]
    fn a_standalone_pull_never_touches_the_diff_baseline() {
        // The job's diff reads the prior run's snapshot, never this store
        // (`docs/portfolio-analysis.md §Triggering`).
        let conn = mem();
        let run = sample_run("run-1", "2026-07-01T00:00:00Z");
        record_run(&conn, &run).unwrap();
        save_pull(&conn, &sample_pull("2026-07-07T12:00:00Z", 999.0)).unwrap();
        let baseline = latest_run(&conn).unwrap().unwrap();
        assert_eq!(baseline, run, "the run snapshot is untouched by a pull");
        assert_eq!(baseline.holdings.positions[0].quantity, 100.0);
    }

    fn sample_record(symbol: &str, created_on: &str) -> crate::portfolio::outcome::PriceRecord {
        use crate::portfolio::outcome::*;
        PriceRecord {
            symbol: symbol.into(),
            created_on: created_on.into(),
            spot: 195.0,
            anchor: Some(crate::portfolio::engine::DatedValue {
                date: "2026-08-03".into(),
                value: 194.0,
            }),
            model: HorizonPrices {
                three_month: Some(200.0),
                twelve_month: None,
                three_year: Some(260.0),
            },
            engine: HorizonPrices {
                three_month: Some(198.0),
                twelve_month: Some(210.0),
                three_year: Some(250.0),
            },
        }
    }

    fn sample_check(horizon: crate::portfolio::outcome::Horizon) -> crate::portfolio::outcome::Check {
        use crate::portfolio::outcome::*;
        Check {
            horizon,
            checked_on: "2026-11-05".into(),
            run_id: "run-2".into(),
            outcome: CheckOutcome::Unscorable {
                cause: UnscorableCause::NoCloseInProximity,
            },
        }
    }

    #[test]
    fn episodes_and_checks_append_and_load_in_insertion_order() {
        use crate::portfolio::outcome::Horizon;
        let conn = mem();
        let first = insert_episode(&conn, &sample_record("aapl", "2026-08-04")).unwrap();
        let second = insert_episode(&conn, &sample_record("MSFT", "2026-08-04")).unwrap();
        assert!(second.id > first.id, "ids are insertion order");
        let check = insert_check(&conn, first.id, &sample_check(Horizon::ThreeMonth)).unwrap();
        let store = load_episode_store(&conn).unwrap();
        assert_eq!(store.episodes, vec![first.clone(), second]);
        assert_eq!(store.checks, vec![check]);
        assert_eq!(store.skipped, 0);
        let symbol: String = conn
            .query_row("SELECT symbol FROM portfolio_episodes WHERE id = ?1", [first.id], |r| r.get(0))
            .unwrap();
        assert_eq!(symbol, "AAPL", "the column is stored uppercase");
    }

    #[test]
    fn a_horizon_takes_one_check_ever() {
        use crate::portfolio::outcome::Horizon;
        let conn = mem();
        let ep = insert_episode(&conn, &sample_record("AAPL", "2026-08-04")).unwrap();
        insert_check(&conn, ep.id, &sample_check(Horizon::ThreeMonth)).unwrap();
        assert!(
            insert_check(&conn, ep.id, &sample_check(Horizon::ThreeMonth)).is_err(),
            "a second check on the same horizon is refused"
        );
        insert_check(&conn, ep.id, &sample_check(Horizon::TwelveMonth)).unwrap();
        assert_eq!(load_episode_store(&conn).unwrap().checks.len(), 2);
    }

    #[test]
    fn an_unreadable_row_is_skipped_logged_and_never_aborts_the_load() {
        use crate::portfolio::outcome::Horizon;
        let conn = mem();
        let ep = insert_episode(&conn, &sample_record("AAPL", "2026-08-04")).unwrap();
        conn.execute(
            "INSERT INTO portfolio_episodes (symbol, created_on, episode_json)
             VALUES ('AAPL', '2026-09-04', '{not json')",
            [],
        )
        .unwrap();
        insert_check(&conn, ep.id, &sample_check(Horizon::ThreeMonth)).unwrap();
        conn.execute(
            "INSERT INTO portfolio_episode_checks (episode_id, horizon, run_id, check_json)
             VALUES (?1, 'twelve_month', 'run-2', '{not json')",
            [ep.id],
        )
        .unwrap();
        let store = load_episode_store(&conn).unwrap();
        assert_eq!(store.episodes, vec![ep.clone()]);
        assert_eq!(store.checks.len(), 1);
        assert_eq!(store.skipped, 2);
        // The unreadable check still holds its horizon written, so it is never
        // re-due.
        assert!(store.written.contains(&(ep.id, Horizon::TwelveMonth)));
        assert!(store.written.contains(&(ep.id, Horizon::ThreeMonth)));
        // Never deleted: the rows stay for inspection.
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM portfolio_episodes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2);
    }

    #[test]
    fn price_bars_merge_by_date_and_load_oldest_first() {
        let conn = mem();
        let bar = |date: &str, close: f64| crate::portfolio::engine::DatedValue {
            date: date.into(),
            value: close,
        };
        merge_price_bars(&conn, "aapl", &[bar("2026-08-03", 195.0), bar("2026-08-01", 193.0)])
            .unwrap();
        // A re-fetch re-bases a shared date (split adjustment): newer wins.
        merge_price_bars(&conn, "AAPL", &[bar("2026-08-03", 19.5), bar("2026-08-04", 19.6)])
            .unwrap();
        let bars = load_price_bars(&conn, "AAPL").unwrap();
        assert_eq!(
            bars,
            vec![bar("2026-08-01", 193.0), bar("2026-08-03", 19.5), bar("2026-08-04", 19.6)]
        );
        assert!(load_price_bars(&conn, "MSFT").unwrap().is_empty());
    }

    #[test]
    fn json_float_round_trip_is_bit_exact() {
        // serde_json's `float_roundtrip` feature is load-bearing in Cargo.toml:
        // without it a parse can drift 1 ulp. This pins it so a dependency edit
        // can't silently reopen the drift.
        let mut checked = 0usize;
        let mut check = |v: f64| {
            let s = serde_json::to_string(&v).unwrap();
            let back: f64 = serde_json::from_str(&s).unwrap();
            assert_eq!(v.to_bits(), back.to_bits(), "float drift on {v:?} via {s}");
            checked += 1;
        };
        for i in 1..5000u32 {
            let f = f64::from(i);
            check(f.sqrt());
            check(f.ln() * 1e-7);
            check(1.0 / f);
            check(f * std::f64::consts::PI);
        }
        for v in [5e-324, 2.225_073_858_507_201_4e-308, f64::MAX, 0.1, 2.0 / 3.0] {
            check(v);
        }
        assert!(checked > 0);
    }
}
