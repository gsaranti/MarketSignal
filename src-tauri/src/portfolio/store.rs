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
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id     TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL,
            run_json   TEXT NOT NULL
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

/// The most recent run, or `None` before any run exists. The prior run's verdicts
/// feed the next run's continuity check (`docs/portfolio-analysis.md` §Continuity and
/// isolation). Newest-first by `created_at` with an `id` tiebreak, matching the
/// report's recent-reports ordering.
pub fn latest_run(conn: &Connection) -> Result<Option<PortfolioRun>> {
    let json = conn
        .query_row(
            "SELECT run_json FROM portfolio_runs
             ORDER BY created_at DESC, id DESC
             LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

/// List the most recent runs, newest first, capped at `limit` — the Portfolio page's
/// run history.
pub fn list_recent_runs(conn: &Connection, limit: u32) -> Result<Vec<PortfolioRun>> {
    let mut stmt = conn.prepare(
        "SELECT run_json FROM portfolio_runs
         ORDER BY created_at DESC, id DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(serde_json::from_str(&row?)?);
    }
    Ok(out)
}

/// One sidebar row of the Portfolio-runs history (`docs/interface.md §Main
/// Layout`; the design package's shared-history `RunRow`): identity, timestamp,
/// and the two counts the row renders — never the run's verdict payload, so the
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
}

/// List the most recent runs' summaries, newest first, capped at `limit` — the
/// sidebar's Portfolio-runs history listing. Same ordering as [`latest_run`] /
/// [`prune_runs`], so the list shows exactly the retained window. The counts
/// come from each stored blob (bounded by the retention cap, so this stays a
/// handful of local parses); the webview never receives the blobs themselves.
pub fn list_run_summaries(conn: &Connection, limit: u32) -> Result<Vec<PortfolioRunSummary>> {
    Ok(list_recent_runs(conn, limit)?
        .into_iter()
        .map(|run| PortfolioRunSummary {
            run_id: run.run_id,
            created_at: run.created_at,
            holdings_count: run.holdings.positions.len(),
            graded_count: run.roll_up.graded_count,
        })
        .collect())
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
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

/// Prune runs beyond the newest `keep`, oldest first — the per-feature retention
/// cascade (`docs/storage.md §Local Analysis Suite Storage`). Same newest-first
/// ordering as [`latest_run`], so it evicts exactly the runs the history no longer
/// shows. Idempotent; a no-op at or under the cap.
pub fn prune_runs(conn: &Connection, keep: u32) -> Result<()> {
    conn.execute(
        "DELETE FROM portfolio_runs
         WHERE id NOT IN (
             SELECT id FROM portfolio_runs
             ORDER BY created_at DESC, id DESC
             LIMIT ?1
         )",
        [keep],
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
            }],
            roll_up: PortfolioRollUp {
                role_risk_only_count: 0,
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                top_position_weight: 0.66,
                cash_weight: 0.34,
                exited: vec![],
                overview: "single fixture holding".into(),
            },
            audit: vec![HoldingAudit {
                target_meta: None,
                symbol: "AAPL".into(),
                metrics: ComputedMetrics::default(),
                sources: vec!["FMP".into()],
                model_ids: vec!["qwen3.5:122b".into()],
                prompt_version: "portfolio-v1".into(),
                degraded_inputs: vec![],
            }],
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
        assert_eq!(latest_run(&conn).unwrap().unwrap().run_id, "run-10");
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
                },
                PortfolioRunSummary {
                    run_id: "run-a".into(),
                    created_at: "2026-07-01T00:00:00Z".into(),
                    holdings_count: 1,
                    graded_count: 0,
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
}
