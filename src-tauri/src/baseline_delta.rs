//! Deterministic change view between two baseline scans: the level-by-level diff of this
//! run's baseline against the most recent prior snapshot (`storage::baseline_snapshots`),
//! computed in Rust so the agent reads an exact, bounded diff rather than re-deriving
//! arithmetic from two raw snapshots. Only the level-bearing [`DELTA_GROUPS`] are diffed;
//! a series present in only one snapshot surfaces as a `new` / `missing` transition, never
//! a fabricated change. The diff carries the actual elapsed interval, since reports are
//! not guaranteed weekly.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::data_sources::{BaselineMarketData, GapReason, GroupKind, Quote};

/// The baseline groups whose levels are diffed report-to-report. Level-bearing and
/// joinable by a stable series id; everything else in the baseline is set-valued or
/// already a return, so it carries no meaningful level delta.
pub const DELTA_GROUPS: [GroupKind; 6] = [
    GroupKind::Indices,
    GroupKind::Internals,
    GroupKind::MacroLevels,
    GroupKind::LaborLevels,
    GroupKind::SectorPe,
    GroupKind::MarketRiskPremium,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Up,
    Down,
    Flat,
}

/// One series' change between the prior report's baseline and this run's, joined by id.
/// `pct_change` is omitted when the prior level was zero or non-finite (no honest
/// percentage exists) — `abs_change` always carries the move.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesDelta {
    pub group: GroupKind,
    pub id: String,
    pub name: String,
    pub current: f64,
    pub prior: f64,
    pub abs_change: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pct_change: Option<f64>,
    pub direction: Direction,
}

/// A series present in exactly one of the two snapshots. No delta is defined, so it is
/// reported as a transition rather than a fabricated change against an implied zero.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesTransition {
    pub group: GroupKind,
    pub id: String,
    pub name: String,
    /// The gap reason that explains the absence, when the missing run's manifest recorded
    /// one. `None` means no gap recorded it — a genuine coverage change, not a data outage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<GapReason>,
}

/// The deterministic change view handed to the agent: how each level-bearing series moved
/// since the previous report, which newly appeared, and which dropped out, over an
/// explicit elapsed interval.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BaselineDeltas {
    /// Days elapsed since the prior report's baseline was captured (fractional): the real
    /// gap, not an assumed week.
    pub elapsed_days: f64,
    pub changed: Vec<SeriesDelta>,
    /// Series present this run but absent from the prior snapshot.
    pub new: Vec<SeriesTransition>,
    /// Series present in the prior snapshot but absent this run.
    pub missing: Vec<SeriesTransition>,
}

/// `(id, name, level)` for each series in `group` — the uniform shape the join works
/// over; groups outside [`DELTA_GROUPS`] yield nothing. Shared with `baseline_history`,
/// so one definition fixes the level-bearing series both views read.
pub(crate) fn series_levels(
    data: &BaselineMarketData,
    group: GroupKind,
) -> Vec<(String, String, f64)> {
    fn from_quotes(quotes: &[Quote]) -> Vec<(String, String, f64)> {
        quotes
            .iter()
            .map(|q| {
                (q.symbol.clone(), q.name.clone(), q.price)
            })
            .collect()
    }
    match group {
        GroupKind::Indices => from_quotes(&data.indices),
        GroupKind::Internals => from_quotes(&data.internals),
        GroupKind::MacroLevels => from_quotes(&data.macro_levels),
        GroupKind::LaborLevels => from_quotes(&data.labor_levels),
        // Sector P/E is exchange-specific, so the join key pins (sector, exchange). A row
        // whose `pe` was band-dropped to `None` has no level to diff, so it surfaces as a
        // `new` / `missing` transition rather than diffing against a fabricated level.
        GroupKind::SectorPe => data
            .sector_pe
            .iter()
            .filter_map(|s| {
                s.pe.map(|pe| {
                    (
                        format!("{}|{}", s.sector, s.exchange),
                        format!("{} ({})", s.sector, s.exchange),
                        pe,
                    )
                })
            })
            .collect(),
        GroupKind::MarketRiskPremium => data
            .market_risk_premium
            .iter()
            .map(|m| {
                (
                    m.country.clone(),
                    format!("{} equity risk premium", m.country),
                    m.total_equity_risk_premium,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A `(group, series_id) -> reason` lookup over one snapshot's gap manifest. Keyed the way
/// the adapters record gaps (the provider series id, which doubles as the quote symbol);
/// composite-key groups (sector P/E, MRP) won't match and degrade to no reason.
fn gap_reasons(data: &BaselineMarketData) -> HashMap<(GroupKind, &str), GapReason> {
    data.gaps
        .iter()
        .map(|g| ((g.group, g.series_id.as_str()), g.reason))
        .collect()
}

/// The level-by-level change of `current` against `prior` over `elapsed_days`. A series
/// in both snapshots becomes a [`SeriesDelta`]; one present in only `current` is `new`,
/// only `prior` is `missing`, tagged with the gap reason from the missing run's manifest
/// when one exists.
pub fn compute_deltas(
    current: &BaselineMarketData,
    prior: &BaselineMarketData,
    elapsed_days: f64,
) -> BaselineDeltas {
    let mut changed = Vec::new();
    let mut new = Vec::new();
    let mut missing = Vec::new();

    // A series `missing` this run is absent from `current`, so its reason (if any) lives in
    // the current manifest; one that's `new` this run was absent from `prior`, so its prior
    // gap explains whether it newly appeared or merely recovered from a prior outage.
    let current_gaps = gap_reasons(current);
    let prior_gaps = gap_reasons(prior);

    for group in DELTA_GROUPS {
        let cur = series_levels(current, group);
        let pri = series_levels(prior, group);
        let prior_by_id: HashMap<&str, f64> =
            pri.iter().map(|(id, _, v)| (id.as_str(), *v)).collect();
        let current_ids: HashSet<&str> = cur.iter().map(|(id, _, _)| id.as_str()).collect();

        for (id, name, current_level) in &cur {
            match prior_by_id.get(id.as_str()) {
                Some(&prior_level) => {
                    let abs_change = current_level - prior_level;
                    let pct_change = if prior_level != 0.0
                        && prior_level.is_finite()
                        && current_level.is_finite()
                    {
                        Some(abs_change / prior_level * 100.0)
                    } else {
                        None
                    };
                    let direction = if abs_change > 0.0 {
                        Direction::Up
                    } else if abs_change < 0.0 {
                        Direction::Down
                    } else {
                        Direction::Flat
                    };
                    changed.push(SeriesDelta {
                        group,
                        id: id.clone(),
                        name: name.clone(),
                        current: *current_level,
                        prior: prior_level,
                        abs_change,
                        pct_change,
                        direction,
                    });
                }
                None => new.push(SeriesTransition {
                    group,
                    id: id.clone(),
                    name: name.clone(),
                    reason: prior_gaps.get(&(group, id.as_str())).copied(),
                }),
            }
        }

        for (id, name, _) in &pri {
            if !current_ids.contains(id.as_str()) {
                missing.push(SeriesTransition {
                    group,
                    id: id.clone(),
                    name: name.clone(),
                    reason: current_gaps.get(&(group, id.as_str())).copied(),
                });
            }
        }
    }

    BaselineDeltas {
        elapsed_days,
        changed,
        new,
        missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_sources::{Change, MarketRiskPremium, SectorPe};

    fn quote(symbol: &str, name: &str, price: f64) -> Quote {
        Quote {
            symbol: symbol.into(),
            name: name.into(),
            price,
            change: Change::percent(0.0),
            unit: "index points".into(),
        }
    }

    #[test]
    fn computes_rise_fall_new_and_missing() {
        let prior = BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 5_500.0)],
            internals: vec![
                quote("^VIX", "CBOE Volatility Index", 14.0),
                quote("^TNX", "10-Year Treasury Yield", 4.2),
            ],
            ..Default::default()
        };
        let current = BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 5_610.0)],
            internals: vec![
                quote("^VIX", "CBOE Volatility Index", 13.0),
                // ^TNX dropped out; ^DXY appeared.
                quote("^DXY", "US Dollar Index", 99.0),
            ],
            ..Default::default()
        };

        let d = compute_deltas(&current, &prior, 6.0);
        assert_eq!(d.elapsed_days, 6.0);

        let sp = d.changed.iter().find(|s| s.id == "^GSPC").unwrap();
        assert!((sp.abs_change - 110.0).abs() < 1e-9);
        assert!((sp.pct_change.unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(sp.direction, Direction::Up);

        let vix = d.changed.iter().find(|s| s.id == "^VIX").unwrap();
        assert!((vix.abs_change + 1.0).abs() < 1e-9);
        assert_eq!(vix.direction, Direction::Down);

        assert!(d.new.iter().any(|t| t.id == "^DXY"));
        assert!(d.missing.iter().any(|t| t.id == "^TNX"));
        // A dropped/added series is never a fabricated change.
        assert!(!d.changed.iter().any(|s| s.id == "^TNX" || s.id == "^DXY"));
    }

    /// Pins the name pass-through on both `from_quotes` outputs (a changed delta and a
    /// `new` transition) so a future change can't silently re-mangle the name.
    #[test]
    fn carries_the_quote_name_into_the_delta_view_verbatim() {
        let prior = BaselineMarketData {
            internals: vec![quote("DGS10", "10-Year Treasury Yield", 4.2)],
            ..Default::default()
        };
        let current = BaselineMarketData {
            internals: vec![
                quote("DGS10", "10-Year Treasury Yield", 4.3),
                quote("NFCI", "Chicago Fed NFCI", -0.4),
            ],
            ..Default::default()
        };
        let d = compute_deltas(&current, &prior, 7.0);

        let tnx = d.changed.iter().find(|s| s.id == "DGS10").unwrap();
        assert_eq!(
            tnx.name, "10-Year Treasury Yield",
            "name carried verbatim on a changed delta"
        );
        assert!(
            (tnx.abs_change - 0.1).abs() < 1e-9,
            "the move is still carried by abs_change"
        );

        let nfci = d.new.iter().find(|t| t.id == "NFCI").unwrap();
        assert_eq!(
            nfci.name, "Chicago Fed NFCI",
            "name carried verbatim on a new transition"
        );
    }

    #[test]
    fn zero_prior_level_omits_pct_but_keeps_abs() {
        let prior = BaselineMarketData {
            macro_levels: vec![quote("NETEXP", "Net Exports", 0.0)],
            ..Default::default()
        };
        let current = BaselineMarketData {
            macro_levels: vec![quote("NETEXP", "Net Exports", 0.5)],
            ..Default::default()
        };
        let d = compute_deltas(&current, &prior, 0.04);
        let m = d.changed.iter().find(|s| s.id == "NETEXP").unwrap();
        assert!((m.abs_change - 0.5).abs() < 1e-9);
        assert_eq!(m.pct_change, None);
        assert_eq!(m.direction, Direction::Up);
    }

    /// The model, not this layer, judges whether so small a window makes the move
    /// meaningful.
    #[test]
    fn elapsed_passes_through_for_a_rapid_regeneration() {
        let prior = BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 5_500.0)],
            ..Default::default()
        };
        let current = BaselineMarketData {
            indices: vec![quote("^GSPC", "S&P 500", 5_500.4)],
            ..Default::default()
        };
        let one_hour = 1.0 / 24.0;
        let d = compute_deltas(&current, &prior, one_hour);
        assert!((d.elapsed_days - one_hour).abs() < 1e-9);
        assert_eq!(d.changed.len(), 1);
    }

    #[test]
    fn valuation_groups_join_and_sectors_are_excluded() {
        use crate::data_sources::SectorPerformance;
        let prior = BaselineMarketData {
            sector_pe: vec![SectorPe {
                sector: "Technology".into(),
                exchange: "NASDAQ".into(),
                pe: Some(30.0),
                pe_history: None,
                trailing_return: None,
            }],
            market_risk_premium: vec![MarketRiskPremium {
                country: "United States".into(),
                country_risk_premium: 0.0,
                total_equity_risk_premium: 4.5,
            }],
            sectors: vec![SectorPerformance {
                sector: "Technology".into(),
                change_pct: 1.2,
            }],
            ..Default::default()
        };
        let current = BaselineMarketData {
            sector_pe: vec![SectorPe {
                sector: "Technology".into(),
                exchange: "NASDAQ".into(),
                pe: Some(31.5),
                pe_history: None,
                trailing_return: None,
            }],
            market_risk_premium: vec![MarketRiskPremium {
                country: "United States".into(),
                country_risk_premium: 0.0,
                total_equity_risk_premium: 4.7,
            }],
            sectors: vec![SectorPerformance {
                sector: "Technology".into(),
                change_pct: -0.8,
            }],
            ..Default::default()
        };
        let d = compute_deltas(&current, &prior, 7.0);
        let pe = d
            .changed
            .iter()
            .find(|s| s.id == "Technology|NASDAQ")
            .unwrap();
        assert!((pe.abs_change - 1.5).abs() < 1e-9);
        let erp = d
            .changed
            .iter()
            .find(|s| s.group == GroupKind::MarketRiskPremium)
            .unwrap();
        assert!((erp.abs_change - 0.2).abs() < 1e-9);
        // `sectors` is a performance percentage — never diffed.
        assert!(!d.changed.iter().any(|s| s.group == GroupKind::Sectors));
    }

    /// History reads are structural, not inter-report levels, so they never enter the diff.
    #[test]
    fn history_reads_leave_the_change_view_unchanged() {
        use crate::data_sources::{IndustrySnapshot, PeHistory, TrailingReturn};
        let sector = |pe: f64| SectorPe {
            sector: "Energy".into(),
            exchange: "NYSE".into(),
            pe: Some(pe),
            pe_history: None,
            trailing_return: None,
        };
        let industry = |change_pct: f64| IndustrySnapshot {
            industry: "Oil & Gas Midstream".into(),
            exchange: "NYSE".into(),
            change_pct,
            pe: Some(20.0),
            pe_history: None,
            trailing_return: None,
        };
        let prior = BaselineMarketData {
            sector_pe: vec![sector(18.0)],
            industries: vec![industry(-0.4)],
            ..Default::default()
        };
        let current = BaselineMarketData {
            sector_pe: vec![sector(17.1)],
            industries: vec![industry(0.7)],
            ..Default::default()
        };
        let plain = compute_deltas(&current, &prior, 7.0);

        let with_history = |mut data: BaselineMarketData, percentile: f64, return_pct: f64| {
            let pe_history = Some(PeHistory {
                percentile,
                min: 16.2,
                median: 21.7,
                max: 35.8,
                start: 21.4,
                latest: 17.1,
                from: "2025-10-13".into(),
                to: "2026-10-12".into(),
            });
            let trailing_return = Some(TrailingReturn {
                return_pct,
                from: "2025-10-13".into(),
                to: "2026-10-12".into(),
            });
            data.sector_pe[0].pe_history = pe_history.clone();
            data.sector_pe[0].trailing_return = trailing_return.clone();
            data.industries[0].pe_history = pe_history;
            data.industries[0].trailing_return = trailing_return;
            data
        };
        let enriched = compute_deltas(
            &with_history(current, 6.5, 31.9),
            &with_history(prior, 40.0, 12.0),
            7.0,
        );
        assert_eq!(enriched, plain);
        assert_eq!(plain.changed.len(), 1, "the sector P/E level alone is diffed");
        assert!(!plain.changed.iter().any(|s| s.group == GroupKind::Industries));
    }

    /// The primary-market group carries its own recent-vs-prior trend, so the change view
    /// never diffs it.
    #[test]
    fn the_primary_market_group_stays_out_of_the_change_view() {
        use crate::data_sources::{ActivityWindow, IpoActivity, PrimaryMarketActivity};
        let level = |price: f64| Quote {
            symbol: "^GSPC".into(),
            name: "S&P 500".into(),
            price,
            change: Change::percent(0.4),
            unit: "index points".into(),
        };
        let prior = BaselineMarketData {
            indices: vec![level(5_400.0)],
            ..Default::default()
        };
        let current = BaselineMarketData {
            indices: vec![level(5_500.0)],
            ..Default::default()
        };
        let plain = compute_deltas(&current, &prior, 7.0);
        let with_group = |mut data: BaselineMarketData, recent_count: u32| {
            let window = || ActivityWindow {
                from: "2026-10-05".into(),
                to: "2026-10-11".into(),
            };
            data.primary_market = Some(PrimaryMarketActivity {
                recent: window(),
                prior: window(),
                upcoming: window(),
                ipos: Some(IpoActivity {
                    recent_count,
                    prior_count: 3,
                    upcoming_count: 2,
                    notable: Vec::new(),
                }),
                deals: None,
            });
            data
        };
        let enriched = compute_deltas(&with_group(current, 9), &with_group(prior, 2), 7.0);
        assert_eq!(enriched, plain);
        assert_eq!(plain.changed.len(), 1, "the index level alone is diffed");
        assert!(!plain
            .changed
            .iter()
            .any(|s| s.group == GroupKind::PrimaryMarket));
    }

    /// A transient feed outage must not read as the series leaving the market.
    #[test]
    fn missing_series_carries_its_gap_reason_when_the_manifest_explains_it() {
        use crate::data_sources::DataGap;
        let prior = BaselineMarketData {
            internals: vec![
                quote("DTWEXBGS", "Dollar Index", 99.0),
                quote("DGS10", "10-Year Treasury Yield", 4.2),
            ],
            ..Default::default()
        };
        let current = BaselineMarketData {
            // DTWEXBGS failed to fetch this run: absent from the levels, recorded as a gap.
            internals: vec![quote("DGS10", "10-Year Treasury Yield", 4.25)],
            gaps: vec![DataGap::new(
                GroupKind::Internals,
                "DTWEXBGS",
                "Dollar Index",
                GapReason::Unavailable,
            )],
            ..Default::default()
        };
        let d = compute_deltas(&current, &prior, 7.0);
        let m = d.missing.iter().find(|t| t.id == "DTWEXBGS").unwrap();
        assert_eq!(m.reason, Some(GapReason::Unavailable));
    }

    #[test]
    fn genuinely_absent_series_has_no_reason() {
        let prior = BaselineMarketData {
            internals: vec![quote("DGS2", "2-Year Treasury Yield", 4.0)],
            ..Default::default()
        };
        let current = BaselineMarketData::default();
        let d = compute_deltas(&current, &prior, 7.0);
        let m = d.missing.iter().find(|t| t.id == "DGS2").unwrap();
        assert_eq!(m.reason, None);
    }
}
