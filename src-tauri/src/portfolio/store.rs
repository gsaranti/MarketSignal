//! SQLite persistence for Portfolio Analysis runs (`docs/storage.md §Local Analysis
//! Suite Storage`). House style mirrors [`crate::storage`]: free functions over
//! `&rusqlite::Connection`, the table created by [`init_schema`] (called from
//! `storage::init_schema` so any run path provisions it).
//!
//! A run is persisted whole as one JSON blob — the [`crate::portfolio::PortfolioRun`]
//! carrying the holdings snapshot, the per-holding verdicts, the roll-up, and the
//! audit record — plus the queryable columns the UI lists on (`created_at`). Per-job
//! retention keeps the most recent [`crate::portfolio::PORTFOLIO_RUN_RETENTION`]
//! runs, pruned independently of the report retention and of Trade Opportunities.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::portfolio::{PortfolioRun, PORTFOLIO_RUN_RETENTION};
use crate::schwab::Holdings;

/// Create the Portfolio Analysis tables if absent. Idempotent, like the rest of
/// `storage::init_schema`, which calls this. `holdings_pulls` is a single-row
/// latest-only store (the `CHECK (id = 1)` pins it), matching its
/// most-recent-pull-only semantics.
///
/// Both tables are exported by data portability: a new constraint here needs a
/// matching import pre-check in `portability::import_archive` (see
/// `storage::init_schema`'s coupling note). Today's mirror: `run_id` UNIQUE and
/// the single-row `holdings_pulls` CHECK.
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
    // The quick check's between-run state (`docs/portfolio-analysis.md §The quick
    // check`): a single latest row, deliberately separate from `portfolio_runs` —
    // a quick check must never surface in the run history or become the next full
    // run's diff baseline / ledger-carry source. Exported by data portability
    // (format v2): its flags and breach streaks are durable analytical state that
    // does not regenerate on the next sweep.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_quick_checks (
            id         INTEGER PRIMARY KEY CHECK (id = 1),
            checked_at TEXT NOT NULL,
            state_json TEXT NOT NULL
        )",
        [],
    )?;
    // The outcome-learning decision-episode store (`docs/portfolio-analysis.md
    // §Outcome learning`) — persisted **independent of the run retention** (a
    // 12-month outcome window can outlive it): active episodes are never evicted;
    // matured ones freeze under their own cap. Exported by data portability
    // (format v3); the UNIQUE episode_id is mirrored by an import pre-check.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_outcome_episodes (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            episode_id   TEXT NOT NULL UNIQUE,
            symbol       TEXT NOT NULL,
            anchor_at    TEXT NOT NULL,
            state        TEXT NOT NULL,
            episode_json TEXT NOT NULL
        )",
        [],
    )?;
    // The shared price-bar cache (`docs/storage.md §Local Analysis Suite Storage`)
    // — split-adjusted daily closes keyed by symbol, the label-time strict rule's
    // read/refresh surface. Exported by data portability (format v3) so imported
    // pending episodes can mature offline; the (symbol, date) primary key is
    // mirrored by an import pre-check.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS price_bars (
            symbol TEXT NOT NULL,
            date   TEXT NOT NULL,
            close  REAL NOT NULL,
            PRIMARY KEY (symbol, date)
        )",
        [],
    )?;
    // The per-topic distilled-findings layer (`docs/portfolio-analysis.md`
    // §Starting parameters — Research reuse): one distilled object per
    // (symbol, topic), the rolling per-holding state that seeds the next
    // run's research loop. Survives independently of run retention (the
    // ~4-week seed gate filters at read time — a dormant topic's object is
    // kept, aging by its own vintage). Job-partitioned by construction
    // (Portfolio's layer only). Exported by data portability (format v4).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS portfolio_research_seeds (
            symbol    TEXT NOT NULL,
            topic_key TEXT NOT NULL,
            vintage   TEXT NOT NULL,
            seed_json TEXT NOT NULL,
            PRIMARY KEY (symbol, topic_key)
        )",
        [],
    )?;
    // The interrupted-run checkpoint store (`docs/portfolio-analysis.md §Failure
    // posture` — per-holding checkpoint/resume): one header row — the pinned run
    // identity, holdings pull, Step-5 shared context, version stamps, and the
    // run-level accumulators — plus one row per completed holding. At most one
    // interrupted run exists (the single global run slot); a new run discards
    // the lot as it opens its own trail (a new run failing before that point —
    // holdings pull, rate anchors — leaves the prior trail intact and still
    // resumable) and a successful persist clears its own. Deliberately
    // **excluded from the data-portability archive** (ruled 2026-08-21):
    // transient machine-local operational state that expires at the resume
    // window, not durable analytical data — so no import pre-check mirrors it.
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
/// posture`): the interrupted run's identity, its original normalized holdings
/// pull (held option positions included), the Step-5 shared context, the
/// selective work-list and tail sweep, and the version stamps a resume must
/// match — so completed and resumed holdings alike compute against one book and
/// one run-level context, and the finished run is honestly stamped with the
/// pinned pull's as-of time. Per-holding retrieval — the option-chain fetches
/// included — is deliberately not pinned: it runs live at each holding's own
/// analysis inside any run (`docs/portfolio-workflow.md` §Step 2), so a resumed
/// holding's evidence is fresher than the pinned as-of by up to the resume
/// window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointHeader {
    pub run_id: String,
    /// The interrupted run's `created_at` — the pinned as-of the resume window
    /// ages against.
    pub created_at: String,
    /// The diff / carry baseline at run start (`latest_run`'s id then). Resume
    /// refuses when the store's latest run has moved — any run that persisted
    /// discarded prior checkpoints on its way, so a mismatch means stale state.
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
    /// A selective run's work-list (uppercased); `None` = the whole book. A
    /// selective re-analysis checkpoints identically.
    pub work_list: Option<Vec<String>>,
    /// The selective tail sweep's states, pinned with the run.
    pub swept_tail: Vec<crate::portfolio::quick_check::HoldingQuickState>,
    /// Version stamps at run start — a resume refuses on any mismatch with the
    /// current binary / roster rather than mixing verdicts across contracts.
    pub prompt_version: String,
    pub grade_parameter_version: String,
    pub target_parameter_version: String,
    pub pre_profit_parameter_version: String,
    pub model_ids: Vec<String>,
}

/// The run-level accumulators the post-loop consumers read (data health, episode
/// sector identities, prompt-header names) — re-written beside each holding
/// checkpoint so completed holdings' contributions survive a failure.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckpointAccumulators {
    pub deep_history_failures: usize,
    pub benchmark_gaps: Vec<String>,
    pub sector_by_symbol:
        std::collections::HashMap<String, crate::portfolio::outcome::SectorIdentity>,
    pub industry_by_symbol: std::collections::HashMap<String, Option<String>>,
    pub profile_name_by_symbol: std::collections::HashMap<String, Option<String>>,
}

/// One completed holding's checkpoint — exactly what the loop pushed for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointHolding {
    pub verdict: crate::portfolio::HoldingVerdict,
    pub audit: crate::portfolio::HoldingAudit,
}

/// A loaded checkpoint: header, accumulators, and the completed-holding rows.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub header: CheckpointHeader,
    pub accumulators: CheckpointAccumulators,
    pub holdings: Vec<CheckpointHolding>,
}

/// Open (or reopen) the checkpoint header for a starting run. Fresh accumulators.
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

/// Persist one completed holding's checkpoint row (idempotent per symbol — a
/// resumed run re-checkpointing a symbol replaces its row) together with the
/// refreshed run-level accumulators, in **one transaction**: a crash between
/// the two writes could otherwise restore the verdict without its accumulator
/// contribution, or the contribution without its row.
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

/// Load the interrupted run's checkpoint, if one exists. At most one header row
/// exists (a new run discards before opening its own); an unreadable header or
/// row is loud-skipped — a corrupt checkpoint costs the resume offer, never an
/// error surface (the intact new-run path is always available).
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

/// Discard every checkpoint — a new run's entry path, and a completed resume's
/// cleanup (`docs/portfolio-analysis.md §Failure posture`: a new run discards
/// any interrupted run's checkpoints; its partial verdicts never became a run).
pub fn clear_checkpoints(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM portfolio_checkpoint_holdings", [])?;
    conn.execute("DELETE FROM portfolio_checkpoints", [])?;
    Ok(())
}

// ---- Outcome-episode store (`docs/portfolio-analysis.md §Outcome learning`) ------

/// A row whose JSON no longer decoded at load, identified by its readable SQL
/// columns — enough for the recovery seam ([`crate::portfolio::outcome::
/// lost_active_symbols`]) to re-seed tracking without ever touching the row.
#[derive(Debug, Clone, PartialEq)]
pub struct SkippedEpisodeRow {
    pub episode_id: String,
    pub symbol: String,
    pub anchor_at: String,
    /// The SQL `state` column value ("active" / "matured").
    pub state: String,
    /// How many **readable** episodes preceded this row in the `id`-ordered scan —
    /// its position in insertion order, without exposing SQL ids.
    ///
    /// It is what lets "is this corrupt row superseded?" be answered in insertion
    /// order: any readable episode at index `>= readable_before` was inserted after
    /// it. Answered by comparing `anchor_at` instead, a backwards clock step made a
    /// later-inserted recovery episode look older, so the symbol stayed flagged lost
    /// and re-debuted on every subsequent run.
    pub readable_before: usize,
}

/// A whole-store episode load: the decodable episodes plus the rows that were
/// skipped.
pub struct EpisodeLoad {
    pub episodes: Vec<crate::portfolio::outcome::DecisionEpisode>,
    pub skipped: Vec<SkippedEpisodeRow>,
}

/// Load every decision episode, active and matured, oldest anchor first. A row
/// whose JSON no longer decodes is **skipped, logged, and reported — never a
/// load failure and never deleted**: aborting on one corrupt row would hand the
/// job an empty set (whose never-seeded rule then re-debuts the whole book on
/// every run beside the valid history), while auto-deleting would let a serde
/// regression silently destroy the store. The skipped rows' readable SQL columns
/// ride back so the job can re-seed a symbol whose *active* episode was lost.
/// Bounded by the matured archive's cap plus the active set (~a year of decision
/// changes), so a whole-store load stays a modest local parse.
pub fn load_episodes(conn: &Connection) -> Result<EpisodeLoad> {
    let mut stmt = conn.prepare(
        // **Insertion order** (`id`), not `anchor_at`: run identity in this store is
        // insertion order everywhere since the piece-3 batch, and the in-memory
        // "latest episode" selections read this vec's own order. Ordered by a wall
        // clock, a backwards clock step would place a newly opened episode BEFORE an
        // older active one and permanently shadow it — every later extension,
        // falsifier event and inherited sector identity attaching to the stale
        // predecessor. `anchor_at` stays the episode's dated anchor; it is not its
        // identity.
        "SELECT episode_id, symbol, anchor_at, state, episode_json \
         FROM portfolio_outcome_episodes ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut episodes = Vec::new();
    let mut skipped = Vec::new();
    for row in rows {
        let (episode_id, symbol, anchor_at, state, json) = row?;
        match serde_json::from_str(&json) {
            Ok(episode) => episodes.push(episode),
            Err(e) => {
                eprintln!(
                    "outcome learning: skipping unreadable episode row {episode_id}: {e}"
                );
                skipped.push(SkippedEpisodeRow {
                    episode_id,
                    symbol,
                    anchor_at,
                    state,
                    readable_before: episodes.len(),
                });
            }
        }
    }
    Ok(EpisodeLoad { episodes, skipped })
}

/// Upsert one episode by its stable `episode_id` (open, extend, tag, and label
/// mutations all land through here).
pub fn save_episode(
    conn: &Connection,
    episode: &crate::portfolio::outcome::DecisionEpisode,
) -> Result<()> {
    let state = match episode.state {
        crate::portfolio::outcome::EpisodeState::Active => "active",
        crate::portfolio::outcome::EpisodeState::Matured => "matured",
    };
    let episode_json = serde_json::to_string(episode)?;
    conn.execute(
        "INSERT INTO portfolio_outcome_episodes (episode_id, symbol, anchor_at, state, episode_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(episode_id) DO UPDATE SET
             symbol = excluded.symbol,
             anchor_at = excluded.anchor_at,
             state = excluded.state,
             episode_json = excluded.episode_json",
        params![episode.episode_id, episode.symbol, episode.anchor_at, state, episode_json],
    )?;
    Ok(())
}

/// Prune **matured** episodes beyond the newest `keep` (by anchor), oldest first —
/// the matured archive's cap. Active episodes are never evicted: one still
/// accruing labels is age-bounded, not count-capped.
// Keeps the newest `keep` matured rows by **insertion order** (`id`), matching
// `load_episodes`. Under `anchor_at` a backwards clock step could delete the
// just-matured row while keeping an older one.
pub fn prune_matured_episodes(conn: &Connection, keep: u32) -> Result<()> {
    conn.execute(
        "DELETE FROM portfolio_outcome_episodes
         WHERE state = 'matured' AND id NOT IN (
             SELECT id FROM portfolio_outcome_episodes
             WHERE state = 'matured'
             ORDER BY id DESC
             LIMIT ?1
         )",
        [keep],
    )?;
    Ok(())
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

/// Persist the merged quick-check state, replacing any prior row — latest-only,
/// like `holdings_pulls`.
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

/// The latest quick-check state, or `None` before any quick check ran (or after a
/// full pass cleared it).
///
/// Decode is **loud-skip**, like [`latest_run`]: an unparseable state logs one
/// stderr warning and reads as `None`, never an `Err` — so a corrupt row neither
/// blanks the Portfolio page load (the `latest_quick_check` command) nor, at the
/// job, silently drops the quick-check chaining behind a swallowed error. Only a
/// store read error (SQL / query failure) is an `Err`; the job degrades that the
/// same way, logged (`job::prior_state_read`).
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

/// Drop the quick-check state — the successful full pass's clear-and-acknowledge
/// leg (`docs/portfolio-analysis.md §The quick check`: the pass consumes the
/// triggering observations, so the flags and accumulated evidence events end with
/// it).
pub fn clear_quick_check(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM portfolio_quick_checks", [])?;
    Ok(())
}

/// The latest standalone **Pull holdings** snapshot (`docs/portfolio-analysis.md
/// §Triggering`, `docs/storage.md §Local Analysis Suite Storage`) — **view-only**
/// Portfolio-page state, distinct from the holdings snapshot persisted *inside* each
/// run: the run's snapshot is the holdings-diff baseline and the audit record's
/// basis, while this store is never read by the job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldingsPull {
    /// Canonical UTC RFC3339; the frontend renders local time.
    pub pulled_at: String,
    pub holdings: Holdings,
}

// ---- The per-topic distilled-findings layer (research reuse) ----------------

/// Persist one holding's fresh per-topic layer — the reduce-reconciled topic
/// objects that become the next run's seeds. INSERT OR REPLACE per topic: an
/// analyzed topic's object is rewritten whole; a dormant topic's row is left
/// untouched only when the layer re-emitted it reconciled — a topic the
/// distillation failed to re-emit is deleted instead
/// ([`delete_topic_distillates`]), never left to seed the next run stale.
pub fn save_topic_distillates(
    conn: &Connection,
    symbol: &str,
    layer: &[crate::portfolio::research::TopicDistillate],
) -> Result<()> {
    for t in layer {
        conn.execute(
            "INSERT OR REPLACE INTO portfolio_research_seeds (symbol, topic_key, vintage, seed_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![symbol, t.topic_key, t.vintage, serde_json::to_string(t)?],
        )?;
    }
    Ok(())
}

/// Delete named topic rows for one holding — the reconciliation contract's
/// teeth: an analyzed or dormant topic the distillation failed to re-emit
/// cannot be trusted as reconciled, so its stale row is dropped (the next run
/// seeds that topic cold) rather than surviving unreconciled.
pub fn delete_topic_distillates(
    conn: &Connection,
    symbol: &str,
    topic_keys: &[String],
) -> Result<()> {
    for key in topic_keys {
        conn.execute(
            "DELETE FROM portfolio_research_seeds WHERE symbol = ?1 AND topic_key = ?2",
            params![symbol, key],
        )?;
    }
    Ok(())
}

/// Load one holding's whole per-topic layer (every stored topic object; the
/// seed gate filters expiry at assembly). A corrupt row is skipped with a log
/// line, never a failed load — the loop just runs that topic cold.
pub fn load_topic_distillates(
    conn: &Connection,
    symbol: &str,
) -> Result<Vec<crate::portfolio::research::TopicDistillate>> {
    let mut stmt = conn.prepare(
        "SELECT seed_json FROM portfolio_research_seeds WHERE symbol = ?1 ORDER BY topic_key",
    )?;
    let rows = stmt
        .query_map(params![symbol], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|json| match serde_json::from_str(&json) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("skipping a corrupt research-seed row for {symbol}: {e}");
                None
            }
        })
        .collect())
}

/// Persist a standalone pull, replacing any prior one — the store holds only the
/// most recent snapshot.
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

/// The latest standalone pull, or `None` before any pull happened.
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

/// Insert one completed run. The whole [`PortfolioRun`] is serialized into
/// `run_json`; `run_id` and `created_at` are projected into columns for listing and
/// ordering. The unique `run_id` makes a re-insert of the same run a clean error
/// rather than a silent duplicate.
pub fn insert_run(conn: &Connection, run: &PortfolioRun) -> Result<()> {
    let run_json = serde_json::to_string(run)?;
    conn.execute(
        "INSERT INTO portfolio_runs (run_id, created_at, run_json)
         VALUES (?1, ?2, ?3)",
        params![run.run_id, run.created_at, run_json],
    )?;
    Ok(())
}

/// The most recent run, or `None` before any exists. The prior run's verdicts
/// feed the next run's continuity check
/// (`docs/portfolio-analysis.md` §Continuity and isolation). Newest-first by
/// **insertion order** (`id` — monotonic within a store, and preserved across
/// machines by the portability export's id-order): the production identity of
/// "latest" is the run most recently PERSISTED, never the wall clock's claim —
/// under a backwards clock step a `created_at` ordering would hand the diff/carry
/// baseline (and the page's refresh) back to the prior run while the
/// just-persisted one sat invisible. `created_at` stays display data.
///
/// Decode is **loud-skip**: an unparseable blob logs one stderr warning naming
/// the row and the walk continues to the next row, instead of erroring the
/// whole read — the job caller fails soft to "no prior run", so an `Err` here
/// silently became no diff baseline, no carries, every episode re-debuting on
/// the strength of one corrupt row. What remains an `Err` is a store read error
/// (SQL / query failure); the job degrades that the same way, logged
/// (`job::prior_state_read`), and the page commands surface it.
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

/// Decode one retained run blob, **loud-skip** on failure: single-homed here so a
/// corrupt row degrades to one stderr warning instead of erroring its whole
/// surface — unparseable blobs are a deliberately-retained store state, so one
/// must never blank the runs history or the diff baseline for its entire
/// retention life.
fn decode_run(run_id: &str, json: &str) -> Option<PortfolioRun> {
    match serde_json::from_str::<PortfolioRun>(json) {
        Ok(run) => Some(run),
        Err(e) => {
            eprintln!("[portfolio-store] skipping unparseable run {run_id}: {e}");
            None
        }
    }
}

/// List the most recent runs, newest first, capped at `limit` — the Portfolio page's
/// run history. Same insertion-order identity as [`latest_run`]. Decode is the same
/// loud-skip ([`decode_run`]): one corrupt blob costs its own row, never the other
/// twenty-nine for its retention life.
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

/// One sidebar row of the Portfolio-runs history (`docs/interface.md §Main
/// Layout`; the design package's shared-history `RunRow`): identity, timestamp,
/// and the two counts a readable row renders (an unreadable row hides them —
/// its zeros are placeholders) — never the run's verdict payload, so the
/// listing IPC response stays rows, not ten full runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortfolioRunSummary {
    pub run_id: String,
    /// Canonical UTC RFC3339; the frontend renders local time.
    pub created_at: String,
    /// Positions in the run's holdings snapshot.
    pub holdings_count: usize,
    /// Graded verdicts in the run (the roll-up's `graded_count`).
    pub graded_count: usize,
    /// Whether the run's blob decoded. An unreadable row still lists — its
    /// identity is column-backed — so the history shows the row exists instead
    /// of silently shrinking, and the page's empty states can tell an unreadable
    /// store from a never-ran one (Codex round). The counts above are zero on an
    /// unreadable row.
    pub readable: bool,
}

/// List the most recent runs' summaries, newest first, capped at `limit` — the
/// sidebar's Portfolio-runs history listing. Same ordering as [`latest_run`] /
/// [`prune_runs`], so the list shows exactly the retained window. The counts
/// come from each stored blob (bounded by the retention cap, so this stays a
/// handful of local parses); the webview never receives the blobs themselves.
/// An **unreadable** blob still yields a row — identity read from the SQL
/// columns, counts zero, `readable: false` — so the history never silently
/// shrinks over a corrupt row.
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

/// Load one persisted run by id for the historical Portfolio view, or `None`
/// when the id is unknown (e.g. the run was pruned between listing and click —
/// the frontend re-lists rather than erroring).
pub fn run_by_id(conn: &Connection, run_id: &str) -> Result<Option<PortfolioRun>> {
    let json = conn
        .query_row(
            "SELECT run_json FROM portfolio_runs WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    // A corrupt row reads as not-found ([`decode_run`] loud-skips). The
    // summaries still LIST it — column-backed identity, rendered disabled, so
    // the UI never issues this call for it — which leaves only a stale or
    // out-of-band caller reaching here, and the frontend's
    // pruned-between-listing-and-click recovery (re-list) is exactly right.
    Ok(json.and_then(|j| decode_run(run_id, &j)))
}

/// Whether any run row exists at all. [`latest_run`] returns `None` both for a
/// never-ran store and for one whose only rows are unreadable; this is the cheap
/// column read that lets a surface tell the two apart and refuse honestly (a "no
/// runs yet" message over persisted work misdescribes the store).
pub fn any_runs(conn: &Connection) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM portfolio_runs)",
        [],
        |r| r.get(0),
    )?;
    Ok(exists)
}

/// Prune runs beyond the newest `keep`, oldest first — the per-feature retention
/// cascade (`docs/storage.md §Local Analysis Suite Storage`). Same newest-first
/// ordering as [`list_recent_runs`], so it evicts exactly the runs the history no
/// longer shows; degraded runs count against the one cap (no second retention
/// path — ruled 2026-08-11). Idempotent; a no-op at or under the cap.
pub fn prune_runs(conn: &Connection, keep: u32) -> Result<()> {
    // Insertion-order retention, matching [`latest_run`] / [`list_recent_runs`]
    // exactly — so eviction removes precisely the runs the history no longer
    // shows, and the run being persisted (the max id) is inherently inside the
    // keep set even under a backwards wall-clock step (a `created_at` ordering
    // could evict it inside its own transaction, leaving a Successful job
    // pointing at a phantom run_id).
    conn.execute(
        "DELETE FROM portfolio_runs
         WHERE id NOT IN (
             SELECT id FROM portfolio_runs
             ORDER BY id DESC
             LIMIT ?1
         )",
        [keep],
    )?;
    // The evicted runs' per-holding continuity summaries go with them: a
    // Portfolio-namespace `summary` row is keyed `{run_id}:{SYMBOL}`, and
    // run-summary vector rows follow run retention (`docs/storage.md §Local
    // Vector Memory`) — durable `learning` rows are untouched. Matching on the
    // id prefix against the *surviving* runs also sweeps any orphan row whose
    // run never persisted.
    conn.execute(
        "DELETE FROM vector_memory
         WHERE kind = 'summary' AND namespace = 'portfolio'
           AND substr(report_id, 1, instr(report_id, ':') - 1)
               NOT IN (SELECT run_id FROM portfolio_runs)",
        [],
    )?;
    Ok(())
}

/// Persist a run and enforce retention in one step — the call the job makes once a
/// run completes. Insert then prune to [`PORTFOLIO_RUN_RETENTION`].
pub fn record_run(conn: &Connection, run: &PortfolioRun) -> Result<()> {
    insert_run(conn, run)?;
    prune_runs(conn, PORTFOLIO_RUN_RETENTION)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portfolio::{
        engine::ComputedMetrics, AssetClass, HoldingAudit, HoldingVerdict, PortfolioRollUp,
        PositionChange, VerdictDisposition,
    };
    use crate::schwab::{Holdings, Position};

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn topic_distillates_round_trip_replace_per_topic_and_skip_corrupt_rows() {
        use crate::portfolio::research::TopicDistillate;
        let conn = mem();
        let object = |key: &str, vintage: &str| TopicDistillate {
            topic_key: key.into(),
            vintage: vintage.into(),
            summary: format!("{key} summary"),
            claims: vec![],
        };
        save_topic_distillates(
            &conn,
            "AAPL",
            &[object("competitive-position", "2026-08-01T00:00:00+00:00")],
        )
        .unwrap();
        // A re-analyzed topic replaces its row; an untouched topic survives.
        save_topic_distillates(
            &conn,
            "AAPL",
            &[
                object("competitive-position", "2026-08-23T00:00:00+00:00"),
                object("results-revisions", "2026-08-23T00:00:00+00:00"),
            ],
        )
        .unwrap();
        let loaded = load_topic_distillates(&conn, "AAPL").unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(loaded
            .iter()
            .all(|t| t.vintage == "2026-08-23T00:00:00+00:00"));
        // Another symbol's layer is invisible (job-partitioned per holding).
        assert!(load_topic_distillates(&conn, "MSFT").unwrap().is_empty());
        // A corrupt row skips with a log line, never a failed load.
        conn.execute(
            "INSERT OR REPLACE INTO portfolio_research_seeds (symbol, topic_key, vintage, seed_json)
             VALUES ('AAPL', 'broken', 'x', 'not json')",
            [],
        )
        .unwrap();
        assert_eq!(load_topic_distillates(&conn, "AAPL").unwrap().len(), 2);
        // An unreconciled topic's row deletes by name — other topics and other
        // symbols untouched.
        save_topic_distillates(
            &conn,
            "MSFT",
            &[object("competitive-position", "2026-08-23T00:00:00+00:00")],
        )
        .unwrap();
        delete_topic_distillates(&conn, "AAPL", &["competitive-position".to_string()]).unwrap();
        let remaining = load_topic_distillates(&conn, "AAPL").unwrap();
        assert!(remaining.iter().all(|t| t.topic_key != "competitive-position"));
        assert!(remaining.iter().any(|t| t.topic_key == "results-revisions"));
        assert_eq!(load_topic_distillates(&conn, "MSFT").unwrap().len(), 1);
    }

    fn sample_run(run_id: &str, created_at: &str) -> PortfolioRun {
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
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
                thesis_ledger: None,
                analyzed_at: None,
                action_source: Default::default(),
                side_reversed: false,
            }],
            roll_up: PortfolioRollUp {
                role_risk_only_count: 0,
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                top_position_weight: 0.66,
                cash_weight: 0.34,
                exited: vec![],
                data_health: None,
                overview: "single fixture holding".into(),
            },
            audit: vec![HoldingAudit {
                what_changed_audit: None,
                research: None,
                target_meta: None,
                symbol: "AAPL".into(),
                metrics: ComputedMetrics::default(),
                sources: vec!["FMP".into()],
                model_ids: vec!["qwen3.5:122b".into()],
                prompt_version: "portfolio-v1".into(),
                degraded_inputs: vec![],
                action_annotations: vec![],
                grade_parameter_version: None,
                ledger_audit: None,
                quick_basis: None,
                authoring_close: None,
                fund_exposure: None,
                pre_profit: None,
                hurdle: None,
                forensic: None,
                tech_event_pre_flag: None,
                short_interest: None,
                implied_expectations: None,
                narrative: None,
                option_overlay: None,
            }],
            rate_prints: None,
            outcome: None,
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

    #[test]
    fn quick_check_basis_and_rate_prints_round_trip() {
        let conn = mem();
        let mut run = sample_run("run-1", "2026-08-03T12:00:00Z");
        run.rate_prints = Some(crate::portfolio::RatePrints {
            dgs2: 0.04,
            dgs10: 0.045,
            dgs2_as_of: Some("2026-08-01".into()),
            dgs10_as_of: Some("2026-08-01".into()),
            fetched_at: "2026-08-03T12:00:00Z".into(),
        });
        run.audit[0].quick_basis = Some(crate::portfolio::engine::QuickCheckBasis {
            spot: 195.0,
            drivers: [6.0, 6.5, 7.0],
            spread_percentiles: Some([0.002, 0.001, 0.0005]),
            raw_percentiles: Some([25.0, 28.0, 31.0]),
            forward_dividends: 1.0,
            dispersion_floor: 0.05,
            consensus_eps_mid: Some(6.5),
        });
        run.audit[0].fund_exposure = Some(crate::portfolio::fund::FundExposureBasis {
            class_label: "US equity fund".into(),
            expense_ratio: Some(0.0009),
            us_share: Some(0.97),
            top_sector: Some(("Technology".into(), 0.31)),
            structural_flag: Some(false),
        });
        insert_run(&conn, &run).unwrap();
        assert_eq!(latest_run(&conn).unwrap().unwrap(), run);
    }

    #[test]
    fn a_pre_basis_blob_decodes_with_absent_quick_fields() {
        // A run persisted before the quick-check basis existed must decode as the
        // absent-basis path (`docs/portfolio-analysis.md` §The quick check — the
        // rate-dependent families read `unknown` until a full run re-persists).
        let conn = mem();
        let run = sample_run("run-old", "2026-07-31T00:00:00Z");
        let mut value = serde_json::to_value(&run).unwrap();
        value.as_object_mut().unwrap().remove("rate_prints");
        for audit in value["audit"].as_array_mut().unwrap() {
            let a = audit.as_object_mut().unwrap();
            a.remove("quick_basis");
            a.remove("fund_exposure");
        }
        conn.execute(
            "INSERT INTO portfolio_runs (run_id, created_at, run_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![run.run_id, run.created_at, value.to_string()],
        )
        .unwrap();
        let back = latest_run(&conn).unwrap().unwrap();
        assert!(back.rate_prints.is_none());
        assert!(back.audit[0].quick_basis.is_none());
        assert!(back.audit[0].fund_exposure.is_none());
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
        // The oldest run fell off; the newest survives.
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
        // A backwards wall-clock step must not demote the just-persisted run:
        // under a `created_at` ordering it survived the prune (if at all) as an
        // invisible extra row — `latest_run` returned the prior run, the capped
        // history excluded it, the page refresh swapped it away, and the next
        // analysis diffed against the wrong baseline. Insertion order is the
        // production identity everywhere.
        let conn = mem();
        for i in 0..PORTFOLIO_RUN_RETENTION {
            let created_at = format!("2026-06-{:02}T00:00:00Z", i + 2);
            record_run(&conn, &sample_run(&format!("run-{i:02}"), &created_at)).unwrap();
        }
        // The clock steps back before every retained run.
        record_run(&conn, &sample_run("run-backdated", "2026-06-01T00:00:00Z")).unwrap();
        // The newest-inserted run IS the latest, heads the CAPPED history the
        // page actually queries, and the count holds the retention cap.
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
        // The limit caps the window like the report sidebar's.
        assert_eq!(list_run_summaries(&conn, 1).unwrap().len(), 1);
        // A listed id opens the full run; an unknown (pruned) id is None, not an error.
        let back = run_by_id(&conn, "run-a").unwrap().unwrap();
        assert_eq!(back.run_id, "run-a");
        assert!(run_by_id(&conn, "run-gone").unwrap().is_none());
    }

    #[test]
    fn latest_run_skips_an_unparseable_blob_instead_of_erroring() {
        // The callers fail-soft with `.ok().flatten()`, so an `Err` here would
        // silently become "no prior run" — no diff baseline, no carries — on
        // the strength of one corrupt row. The read skips it loudly instead.
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
        // Same loud-skip as `latest_run`: a corrupt quick-check row reads as
        // "no quick check since the last pass" — never an `Err` that would blank
        // the Portfolio page load or, at the job, silently drop the chaining.
        let conn = mem();
        conn.execute(
            "INSERT INTO portfolio_quick_checks (id, checked_at, state_json) \
             VALUES (1, '2026-08-11T00:00:00Z', '{\"not\": \"a state\"}')",
            [],
        )
        .unwrap();
        assert!(latest_quick_check(&conn).unwrap().is_none());
        // A readable state still round-trips (the skip is decode-only).
        let state = crate::portfolio::quick_check::QuickCheckState {
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
        // Unparseable blobs are a retained store state; one must not blank
        // the sidebar for its retention life (combined-range review).
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
        // The summaries still LIST the unreadable row — identity is column-backed
        // — so the history never silently shrinks and the page can tell
        // unreadable from never-ran.
        let summaries = list_run_summaries(&conn, 10).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].run_id, "run-corrupt");
        assert!(!summaries[0].readable);
        assert_eq!(summaries[0].holdings_count, 0);
        assert!(summaries[1].readable);
        // A direct open of the corrupt row reads as not-found: the listing
        // shows it disabled (never a live control), so only a stale or
        // out-of-band call reaches it, and the frontend's re-list recovery
        // is exactly right.
        assert!(run_by_id(&conn, "run-corrupt").unwrap().is_none());
    }

    #[test]
    fn any_runs_splits_never_ran_from_all_unreadable() {
        // `latest_run` returns `None` both for a never-ran store and one whose
        // only rows are unreadable; `any_runs` tells the two apart so a refusal
        // names the true state instead of calling a store of corrupt rows empty.
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
        // The job's holdings diff reads the prior *run's* snapshot (`job.rs` reads
        // `store::latest_run`), never this store — pulling between runs must not
        // change what the diff reports (`docs/portfolio-analysis.md §Triggering`).
        let conn = mem();
        let run = sample_run("run-1", "2026-07-01T00:00:00Z");
        record_run(&conn, &run).unwrap();
        save_pull(&conn, &sample_pull("2026-07-07T12:00:00Z", 999.0)).unwrap();
        let baseline = latest_run(&conn).unwrap().unwrap();
        assert_eq!(baseline, run, "the run snapshot is untouched by a pull");
        assert_eq!(baseline.holdings.positions[0].quantity, 100.0);
    }

    fn sample_episode(episode_id: &str, symbol: &str, anchor_at: &str) -> crate::portfolio::outcome::DecisionEpisode {
        use crate::portfolio::outcome::*;
        DecisionEpisode {
            episode_id: episode_id.into(),
            symbol: symbol.into(),
            anchor_run_id: "run-1".into(),
            anchor_at: anchor_at.into(),
            intrinsic_vintage: anchor_at.into(),
            vintage_fresh: true,
            action_source: Default::default(),
            position_change: PositionChange::New,
            sector: SectorIdentity::resolve(Some("Technology")),
            opened: vec![OpenReason::Debut],
            body: EpisodeBody::RoleRiskOnly(RoleRiskEpisode {
                action: crate::portfolio::Action::Hold,
                degraded_inputs: vec![],
            }),
            observations: vec![],
            alignment: None,
            falsifier_events: vec![],
            labels: pending_labels(
                chrono::NaiveDate::parse_from_str(&anchor_at[..10], "%Y-%m-%d").unwrap(),
            ),
            state: EpisodeState::Active,
            self_correction_count: 0,
        }
    }

    #[test]
    fn episodes_round_trip_and_upsert_by_episode_id() {
        let conn = mem();
        let mut ep = sample_episode("ep-1", "AAPL", "2026-08-04T12:00:00+00:00");
        save_episode(&conn, &ep).unwrap();
        assert_eq!(load_episodes(&conn).unwrap().episodes, vec![ep.clone()]);
        // An upsert replaces in place — no duplicate row.
        ep.state = crate::portfolio::outcome::EpisodeState::Matured;
        ep.alignment = Some(crate::portfolio::outcome::ObservedNetAlignment::Aligned);
        save_episode(&conn, &ep).unwrap();
        let back = load_episodes(&conn).unwrap().episodes;
        assert_eq!(back.len(), 1);
        assert_eq!(back[0], ep);
    }

    #[test]
    fn a_corrupt_episode_row_is_skipped_reported_and_never_aborts_the_load() {
        // One undecodable row must cost only itself: aborting the whole load
        // would hand the job an empty set, whose never-seeded rule then
        // re-debuts the entire book on every run beside the bad row. The
        // same-symbol case matters: the corrupt row here is AAPL's *latest
        // active* episode beside readable older AAPL history, and the reported
        // skipped row is what lets the plan's recovery seam re-seed the symbol.
        let conn = mem();
        let mut older = sample_episode("ep-old", "AAPL", "2025-08-04T12:00:00+00:00");
        older.state = crate::portfolio::outcome::EpisodeState::Matured;
        save_episode(&conn, &older).unwrap();
        conn.execute(
            "INSERT INTO portfolio_outcome_episodes (episode_id, symbol, anchor_at, state, episode_json)
             VALUES ('ep-bad', 'AAPL', '2026-06-01T00:00:00+00:00', 'active', '{not json')",
            [],
        )
        .unwrap();
        let load = load_episodes(&conn).unwrap();
        assert_eq!(load.episodes.len(), 1, "the readable row survives the bad one");
        assert_eq!(load.episodes[0].episode_id, "ep-old");
        assert_eq!(load.skipped.len(), 1);
        assert_eq!(load.skipped[0].episode_id, "ep-bad");
        assert_eq!(load.skipped[0].symbol, "AAPL");
        assert_eq!(load.skipped[0].state, "active");
        assert_eq!(load.skipped[0].anchor_at, "2026-06-01T00:00:00+00:00");
    }

    #[test]
    fn matured_pruning_never_evicts_an_active_episode() {
        let conn = mem();
        // Three matured (oldest first) + one active older than all of them.
        save_episode(&conn, &{
            let mut e = sample_episode("ep-active", "GONE", "2025-01-01T00:00:00+00:00");
            e.state = crate::portfolio::outcome::EpisodeState::Active;
            e
        })
        .unwrap();
        for (id, at) in [
            ("ep-a", "2026-01-01T00:00:00+00:00"),
            ("ep-b", "2026-02-01T00:00:00+00:00"),
            ("ep-c", "2026-03-01T00:00:00+00:00"),
        ] {
            let mut e = sample_episode(id, "AAPL", at);
            e.state = crate::portfolio::outcome::EpisodeState::Matured;
            save_episode(&conn, &e).unwrap();
        }
        prune_matured_episodes(&conn, 2).unwrap();
        let ids: Vec<String> = load_episodes(&conn)
            .unwrap()
            .episodes
            .into_iter()
            .map(|e| e.episode_id)
            .collect();
        // The oldest matured fell; the active row — older still — survives.
        assert_eq!(ids, vec!["ep-active", "ep-b", "ep-c"]);
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
        // Every store in this module persists its value as serde_json text, so a
        // carried numeric survives runs only as print → parse. Without serde_json's
        // `float_roundtrip` feature the parse can drift 1 ulp — which is why
        // carried-verdict tests once had to avoid exact comparison. The feature is
        // load-bearing in Cargo.toml; this pins it so a dependency edit can't
        // silently reopen the drift.
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
