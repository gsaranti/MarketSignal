//! The **price-action confirmation read** — the shared engine's cross-archetype
//! confirmer (`docs/trade-opportunities.md` §Signal inputs, §Starting parameters,
//! the price-action confirmation row; `docs/trade-opportunities-workflow.md`
//! §Step 5c): **relative strength** against the market and against the sector,
//! and **proximity to a multi-year base breakout**, from the dated deep closes. A
//! confirmer, never a trigger — it adjusts conviction and never substitutes for
//! the leading-metric anchor, so nothing here gates a name.
//!
//! Relative strength reuses the momentum factor's 12−1 windows and its proximity
//! rule (`portfolio::factors`); every series is price-only (split-adjusted,
//! dividend-unadjusted), the stock's and the benchmarks' alike. Pure over the
//! closes and the as-of session, so every rule is unit-testable offline.

use serde::{Deserialize, Serialize};

use super::engine::DatedValue;
use super::factors::{close_near, closes_through, dedupe_closes, momentum_as_of, shift};

// ---------------------------------------------------------------------------
// Calibration surface — drafted, calibratable (`docs/trade-opportunities.md`
// §Starting parameters, the price-action confirmation row).
// ---------------------------------------------------------------------------

/// How far back the breakout pivot reaches, in calendar days — three years, a
/// multi-year base the deep history covers (the volume spike at a range edge
/// grows with the time since that edge was last hit — Huddart, Lang and Yetman).
pub const PIVOT_LOOKBACK_DAYS: i64 = 1_095;

/// The base: the sessions before the latest close, O'Neil's five-week minimum
/// base length.
pub const BASE_SESSIONS: usize = 25;

/// The deepest a valid base may sit below the pivot, as its lowest close over
/// the pivot — O'Neil's deepest cup (a third off the high).
pub const BASE_MIN_DEPTH_RATIO: f64 = 0.67;

/// The last close over the pivot at or above which the name is near a breakout,
/// and above which a breakout is extended — O'Neil's buy zone runs to 5% past
/// the pivot, mirrored below it for "near".
pub const NEAR_BREAKOUT_RATIO: f64 = 0.95;
pub const EXTENDED_RATIO: f64 = 1.05;

/// Where the last close sits against the multi-year pivot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BreakoutBand {
    /// Below [`NEAR_BREAKOUT_RATIO`] of the pivot.
    Below,
    /// From [`NEAR_BREAKOUT_RATIO`] up to the pivot.
    Near,
    /// From the pivot to [`EXTENDED_RATIO`] of it.
    BrokenOut,
    /// Above [`EXTENDED_RATIO`] of the pivot.
    Extended,
}

impl BreakoutBand {
    fn of(ratio: f64) -> Self {
        if ratio < NEAR_BREAKOUT_RATIO {
            BreakoutBand::Below
        } else if ratio < 1.0 {
            BreakoutBand::Near
        } else if ratio <= EXTENDED_RATIO {
            BreakoutBand::BrokenOut
        } else {
            BreakoutBand::Extended
        }
    }
}

/// The multi-year base-breakout read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakoutRead {
    /// The highest close over the three years before the base, and its date.
    pub pivot: f64,
    pub pivot_date: String,
    /// The last close over the pivot.
    pub ratio: f64,
    pub band: BreakoutBand,
    /// The base's lowest close over the pivot.
    pub base_depth_ratio: f64,
    /// Whether the base held under the pivot and no deeper than
    /// [`BASE_MIN_DEPTH_RATIO`] of it.
    pub base_valid: bool,
}

/// The price-action confirmation read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceActionRead {
    /// The stock's 12−1 return relative to the market's: `(1 + stock) /
    /// (1 + market) − 1`.
    pub relative_strength_market: Option<f64>,
    /// The same against the sector benchmark.
    pub relative_strength_sector: Option<f64>,
    /// `true` when both relative strengths read above zero, `false` when either
    /// reads at or below it, `None` when a leg is unread and no read leg is at
    /// or below zero.
    pub confirming: Option<bool>,
    pub breakout: Option<BreakoutRead>,
    pub gaps: Vec<String>,
}

/// The price-action read as of `as_of` (an ISO session date) over the stock's
/// dated closes, the market benchmark's and the sector benchmark's, where one
/// is mapped. Closes may arrive in any order and with a session served twice
/// (the last row wins).
pub fn price_action(
    closes: &[DatedValue],
    market: &[DatedValue],
    sector: Option<&[DatedValue]>,
    as_of: &str,
) -> PriceActionRead {
    let closes = dedupe_closes(closes);
    let mut gaps = Vec::new();
    let stock_momentum = momentum_as_of(&closes, as_of);
    if stock_momentum.is_none() {
        gaps.push(
            "relative strength unread: no stock close near a year or a month back".to_string(),
        );
    }
    let relative = |bench: &[DatedValue], label: &str, gaps: &mut Vec<String>| {
        let stock = stock_momentum?;
        let bench_momentum = momentum_as_of(&dedupe_closes(bench), as_of);
        if bench_momentum.is_none() {
            gaps.push(format!(
                "relative strength vs the {label} unread: no benchmark close near a year or a \
                 month back"
            ));
        }
        let ratio = (1.0 + stock) / (1.0 + bench_momentum?) - 1.0;
        ratio.is_finite().then_some(ratio)
    };
    let relative_strength_market = relative(market, "market", &mut gaps);
    let relative_strength_sector = match sector {
        Some(series) => relative(series, "sector", &mut gaps),
        None => {
            gaps.push("relative strength vs the sector unread: no sector benchmark".to_string());
            None
        }
    };
    let legs = [relative_strength_market, relative_strength_sector];
    let confirming = if legs.iter().flatten().any(|r| *r <= 0.0) {
        Some(false)
    } else if legs.iter().all(Option::is_some) {
        Some(true)
    } else {
        None
    };
    let breakout = match breakout_read(&closes, as_of) {
        Ok(read) => Some(read),
        Err(gap) => {
            gaps.push(gap);
            None
        }
    };
    PriceActionRead {
        relative_strength_market,
        relative_strength_sector,
        confirming,
        breakout,
        gaps,
    }
}

/// The breakout read over deduplicated, date-ordered closes; `Err` is the typed
/// gap.
fn breakout_read(closes: &[DatedValue], as_of: &str) -> Result<BreakoutRead, String> {
    let latest = close_near(closes, as_of)
        .ok_or_else(|| format!("breakout unread: no close near {as_of}"))?
        .clone();
    let start = shift(as_of, PIVOT_LOOKBACK_DAYS)
        .ok_or_else(|| format!("breakout unread: the as-of date {as_of} does not parse"))?;
    if close_near(closes, &start).is_none() {
        return Err("breakout unread: the history does not reach three years back".to_string());
    }
    let through = closes_through(closes, as_of);
    if through.len() < BASE_SESSIONS + 2 {
        return Err("breakout unread: too few sessions for a base and a pivot".to_string());
    }
    let last = through.len() - 1;
    let base = &through[last - BASE_SESSIONS..last];
    let pivot_window: Vec<&DatedValue> = through[..last - BASE_SESSIONS]
        .iter()
        .filter(|c| c.date >= start)
        .collect();
    let pivot = pivot_window
        .iter()
        .copied()
        .max_by(|a, b| a.value.total_cmp(&b.value))
        .filter(|p| p.value > 0.0)
        .ok_or("breakout unread: no positive close before the base")?;
    let base_high = base.iter().map(|c| c.value).fold(f64::NEG_INFINITY, f64::max);
    let base_low = base.iter().map(|c| c.value).fold(f64::INFINITY, f64::min);
    let ratio = latest.value / pivot.value;
    let base_depth_ratio = base_low / pivot.value;
    if !(ratio.is_finite() && base_depth_ratio.is_finite()) {
        return Err("breakout unread: a ratio overflowed".to_string());
    }
    Ok(BreakoutRead {
        pivot: pivot.value,
        pivot_date: pivot.date.clone(),
        ratio,
        band: BreakoutBand::of(ratio),
        base_depth_ratio,
        base_valid: base_high <= pivot.value && base_depth_ratio >= BASE_MIN_DEPTH_RATIO,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const AS_OF: &str = "2026-10-13";

    /// Daily closes from `days` calendar days before 2026-10-13 through it, each
    /// read off `f(days_back)`.
    fn series(days: i64, f: impl Fn(i64) -> f64) -> Vec<DatedValue> {
        let end = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        (0..=days)
            .rev()
            .map(|back| DatedValue {
                date: (end - chrono::Duration::days(back)).format("%Y-%m-%d").to_string(),
                value: f(back),
            })
            .collect()
    }

    /// A close that grows linearly by `per_day` toward today from 100 a year
    /// back.
    fn trending(per_day: f64) -> Vec<DatedValue> {
        series(1_200, move |back| 100.0 + per_day * (365.0 - back as f64))
    }

    #[test]
    fn relative_strength_is_the_ratio_of_the_two_12_1_returns() {
        let stock = trending(0.2);
        let market = trending(0.1);
        let read = price_action(&stock, &market, Some(&market), AS_OF);
        // Stock: 100 a year back, 100 + 0.2·335 a month back.
        let stock_m = (100.0 + 0.2 * 335.0) / 100.0 - 1.0;
        let market_m = (100.0 + 0.1 * 335.0) / 100.0 - 1.0;
        let expected = (1.0 + stock_m) / (1.0 + market_m) - 1.0;
        assert!((read.relative_strength_market.unwrap() - expected).abs() < 1e-12);
        assert_eq!(read.relative_strength_sector, read.relative_strength_market);
        assert_eq!(read.confirming, Some(true));
    }

    #[test]
    fn a_lagging_leg_breaks_the_confirmation_and_an_unread_one_withholds_it() {
        let stock = trending(0.2);
        let read = price_action(&stock, &trending(0.1), Some(&trending(0.3)), AS_OF);
        assert!(read.relative_strength_market.unwrap() > 0.0);
        assert!(read.relative_strength_sector.unwrap() < 0.0);
        assert_eq!(read.confirming, Some(false));
        // No sector benchmark: the market leg alone cannot confirm.
        let read = price_action(&stock, &trending(0.1), None, AS_OF);
        assert_eq!(read.confirming, None);
        assert!(read.gaps.iter().any(|g| g.contains("no sector benchmark")));
        // ...but a lagging read leg still breaks it.
        let read = price_action(&stock, &trending(0.3), None, AS_OF);
        assert_eq!(read.confirming, Some(false));
    }

    #[test]
    fn a_benchmark_with_no_close_near_a_window_reads_no_relative_strength() {
        let stock = trending(0.2);
        // The market series stops 41 days back: no close near a month back.
        let market: Vec<DatedValue> = trending(0.1)
            .into_iter()
            .filter(|c| c.date.as_str() <= "2026-09-02")
            .collect();
        let read = price_action(&stock, &market, Some(&trending(0.1)), AS_OF);
        assert_eq!(read.relative_strength_market, None);
        assert!(read.relative_strength_sector.is_some());
        assert_eq!(read.confirming, None);
        assert!(read.gaps.iter().any(|g| g.contains("vs the market unread")), "{:?}", read.gaps);
    }

    /// A three-year pivot of 100 set 400 days back, a base held between 80 and
    /// 98 over the last weeks, and today's close `today`.
    fn based(today: f64) -> Vec<DatedValue> {
        series(1_200, move |back| match back {
            0 => today,
            400 => 100.0,
            1..=40 => 80.0 + (back % 10) as f64 * 2.0,
            _ => 70.0,
        })
    }

    #[test]
    fn the_pivot_is_the_three_year_high_before_the_base_and_the_bands_read_against_it() {
        for (today, band) in [
            (94.9, BreakoutBand::Below),
            (95.0, BreakoutBand::Near),
            (99.9, BreakoutBand::Near),
            (100.0, BreakoutBand::BrokenOut),
            (105.0, BreakoutBand::BrokenOut),
            (105.1, BreakoutBand::Extended),
        ] {
            let read = price_action(&based(today), &trending(0.1), None, AS_OF);
            let b = read.breakout.expect("the read computes");
            assert_eq!(b.pivot, 100.0);
            assert_eq!(b.pivot_date, "2025-09-08");
            assert_eq!(b.band, band, "{today}");
            assert!((b.ratio - today / 100.0).abs() < 1e-12);
            assert!(b.base_valid, "a base between 80 and 98 holds");
            assert!((b.base_depth_ratio - 0.80).abs() < 1e-12);
        }
    }

    #[test]
    fn a_base_above_the_pivot_or_too_deep_is_invalid() {
        // A base session above the pivot: the breakout already happened.
        let above = series(1_200, |back| match back {
            400 => 100.0,
            10 => 101.0,
            _ => 90.0,
        });
        let b = price_action(&above, &trending(0.1), None, AS_OF).breakout.unwrap();
        assert!(!b.base_valid);
        // A base sunk below two-thirds of the pivot.
        let deep = series(1_200, |back| match back {
            400 => 100.0,
            5 => 60.0,
            _ => 90.0,
        });
        let b = price_action(&deep, &trending(0.1), None, AS_OF).breakout.unwrap();
        assert!(!b.base_valid);
        assert!((b.base_depth_ratio - 0.60).abs() < 1e-12);
        // A high older than three years is outside the pivot window.
        let old_high = series(1_200, |back| match back {
            1_100 => 500.0,
            400 => 100.0,
            _ => 90.0,
        });
        let b = price_action(&old_high, &trending(0.1), None, AS_OF).breakout.unwrap();
        assert_eq!(b.pivot, 100.0);
    }

    #[test]
    fn a_short_or_stale_history_reads_no_breakout() {
        // Two years of history: the pivot window cannot reach three years back.
        let short = series(730, |_| 90.0);
        let read = price_action(&short, &trending(0.1), None, AS_OF);
        assert_eq!(read.breakout, None);
        assert!(read.gaps.iter().any(|g| g.contains("three years back")), "{:?}", read.gaps);
        // The latest close 11 days before the as-of date.
        let read = price_action(&based(100.0), &trending(0.1), None, "2026-10-24");
        assert_eq!(read.breakout, None);
        assert!(read.gaps.iter().any(|g| g.contains("no close near")), "{:?}", read.gaps);
    }

    #[test]
    fn a_session_served_twice_counts_once() {
        let mut closes = based(100.0);
        let mut dup = closes.last().unwrap().clone();
        dup.value = 106.0;
        closes.reverse();
        closes.push(dup);
        let b = price_action(&closes, &trending(0.1), None, AS_OF).breakout.unwrap();
        assert_eq!(b.band, BreakoutBand::Extended, "the last row served wins");
    }
}
