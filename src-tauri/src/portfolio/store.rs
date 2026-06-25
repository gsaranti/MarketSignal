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

use crate::portfolio::{PortfolioRun, PORTFOLIO_RUN_RETENTION};

/// Create the Portfolio Analysis run table if absent. Idempotent, like the rest of
/// `storage::init_schema`, which calls this.
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
    Ok(())
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
        VerdictDisposition,
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
            },
            verdicts: vec![HoldingVerdict {
                symbol: "AAPL".into(),
                asset_class: AssetClass::Stock,
                disposition: VerdictDisposition::NotRated {
                    reason: "fixture".into(),
                },
            }],
            roll_up: PortfolioRollUp {
                graded_count: 0,
                not_rated_count: 1,
                insufficient_evidence_count: 0,
                top_position_weight: 0.66,
                cash_weight: 0.34,
                overview: "single fixture holding".into(),
            },
            audit: vec![HoldingAudit {
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
    fn duplicate_run_id_is_rejected() {
        let conn = mem();
        let run = sample_run("dup", "2026-06-25T12:00:00Z");
        insert_run(&conn, &run).unwrap();
        assert!(insert_run(&conn, &run).is_err(), "run_id is unique");
    }
}
