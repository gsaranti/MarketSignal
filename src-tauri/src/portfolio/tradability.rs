//! The **tradability flag** — the shared engine's liquidity read
//! (`docs/trade-opportunities.md` §Starting parameters, the tradability flag row;
//! `docs/trade-opportunities-workflow.md` §Step 5c). Two legs, each banded on its
//! own input: **Amihud illiquidity** (the mean absolute daily return per dollar
//! traded) over the trailing quarter of sessions, and **days-to-cover** as FINRA
//! serves it. The worse leg sets the band, and the band sets the entry gate's
//! **liquidity haircut**, so a small or illiquid name is flagged and discounted
//! rather than silently excluded. A leg whose input is missing cannot trigger —
//! the risk tier's missing-input rule — and a name with neither input reads
//! unflagged with a logged gap, never a fabricated flag.
//!
//! Pure over the dated bars, the FINRA short-interest read and the as-of session,
//! so every rule is unit-testable offline.

use serde::{Deserialize, Serialize};

use super::engine::DailyBar;
use super::factors::{shift, PRICE_PROXIMITY_DAYS};
use crate::finra::ShortInterestRead;

// ---------------------------------------------------------------------------
// Calibration surface — drafted or sourced as labelled below, calibratable
// (`docs/trade-opportunities.md` §Starting parameters, the tradability flag row).
// ---------------------------------------------------------------------------

/// The trailing daily returns the Amihud read averages over — a quarter of
/// sessions. Drafted: Amihud's original measure is annual; a quarter keeps the
/// read current.
pub const AMIHUD_SESSIONS: usize = 63;

/// The fewest positive-volume sessions an Amihud read stands on — Amihud's
/// 200-of-252 admission ratio scaled to the quarter (drafted). The same floor
/// admits the median dollar volume, counted over the sessions that served a
/// volume.
pub const AMIHUD_MIN_VOLUME_DAYS: usize = 50;

/// Amihud illiquidity (×10⁶: decimal return per $1M traded) at or above which
/// the leg flags, and at or above which it is severe. The flag sits at the 75th
/// percentile of the 2020–25 CRSP distribution (Aldridge, arXiv 2607.01377 —
/// about $0.5M a day at a typical daily move), the severe edge near its 87th
/// (about $50k a day); the distribution is sourced, the cut-points drafted.
pub const AMIHUD_FLAGGED: f64 = 0.05;
pub const AMIHUD_SEVERE: f64 = 0.5;

/// FINRA's served days-to-cover at or above which the leg flags, and at or above
/// which it is severe. Drafted: the stock-level average runs about 5.5–8 days
/// (Hong, Li, Ni, Scheinkman and Yan), so the leg flags the outlier, not the
/// typical name.
pub const DAYS_TO_COVER_FLAGGED: f64 = 10.0;
pub const DAYS_TO_COVER_SEVERE: f64 = 20.0;

/// The entry gate's liquidity haircut per band, as a decimal ratio (3pts, 6pts —
/// `docs/trade-opportunities.md` §Starting parameters, the entry asymmetry row).
pub const HAIRCUT_FLAGGED: f64 = 0.03;
pub const HAIRCUT_SEVERE: f64 = 0.06;

/// The tradability band — the worse of the two legs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TradabilityBand {
    Unflagged,
    Flagged,
    Severe,
}

impl TradabilityBand {
    /// The entry gate's liquidity haircut for this band, a decimal ratio.
    pub fn haircut(self) -> f64 {
        match self {
            TradabilityBand::Unflagged => 0.0,
            TradabilityBand::Flagged => HAIRCUT_FLAGGED,
            TradabilityBand::Severe => HAIRCUT_SEVERE,
        }
    }

    fn of(value: f64, flagged: f64, severe: f64) -> Self {
        if value >= severe {
            TradabilityBand::Severe
        } else if value >= flagged {
            TradabilityBand::Flagged
        } else {
            TradabilityBand::Unflagged
        }
    }
}

/// The tradability read: both legs' inputs, the band they set and its haircut,
/// with a typed gap for each leg that could not be read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradabilityRead {
    /// Amihud illiquidity ×10⁶ over the trailing [`AMIHUD_SESSIONS`] returns.
    pub amihud: Option<f64>,
    /// The positive-volume sessions the Amihud read stood on.
    pub amihud_sessions: usize,
    /// The median daily dollar volume (close × volume) over the same window —
    /// reported, never banded.
    pub median_dollar_volume: Option<f64>,
    /// FINRA's served days-to-cover.
    pub days_to_cover: Option<f64>,
    /// The short-interest row's settlement date, where one was read.
    pub short_interest_settlement: Option<String>,
    pub amihud_band: Option<TradabilityBand>,
    pub days_to_cover_band: Option<TradabilityBand>,
    pub band: TradabilityBand,
    /// The band's liquidity haircut, a decimal ratio.
    pub haircut: f64,
    pub gaps: Vec<String>,
}

/// The tradability read as of `as_of` (an ISO session date). The bars may arrive
/// in any order and with a session served twice (the last row wins); the read
/// stands on the bars through `as_of`, and only when the newest sits within
/// [`PRICE_PROXIMITY_DAYS`] of it.
pub fn tradability(
    bars: &[DailyBar],
    short_interest: Option<&ShortInterestRead>,
    as_of: &str,
) -> TradabilityRead {
    let mut gaps = Vec::new();
    let window = match liquidity_window(bars, as_of) {
        Ok(window) => {
            if window.amihud.is_none() {
                gaps.push(format!(
                    "Amihud illiquidity unread: {} positive-volume session(s) of the last \
                     {AMIHUD_SESSIONS} (need {AMIHUD_MIN_VOLUME_DAYS})",
                    window.amihud_sessions
                ));
            }
            window
        }
        Err(gap) => {
            gaps.push(gap);
            LiquidityWindow::default()
        }
    };
    let LiquidityWindow {
        amihud,
        amihud_sessions,
        median_dollar_volume,
    } = window;
    let days_to_cover = short_interest
        .and_then(|s| s.days_to_cover)
        .filter(|d| d.is_finite() && *d >= 0.0);
    if days_to_cover.is_none() {
        gaps.push(match short_interest {
            None => "days-to-cover unread: no FINRA short-interest row".to_string(),
            Some(_) => "days-to-cover unread: the FINRA row served none".to_string(),
        });
    }
    let amihud_band = amihud.map(|a| TradabilityBand::of(a, AMIHUD_FLAGGED, AMIHUD_SEVERE));
    let days_to_cover_band =
        days_to_cover.map(|d| TradabilityBand::of(d, DAYS_TO_COVER_FLAGGED, DAYS_TO_COVER_SEVERE));
    if amihud_band.is_none() && days_to_cover_band.is_none() {
        gaps.push("no tradability input read — unflagged".to_string());
    }
    let band = amihud_band
        .into_iter()
        .chain(days_to_cover_band)
        .max()
        .unwrap_or(TradabilityBand::Unflagged);
    TradabilityRead {
        amihud,
        amihud_sessions,
        median_dollar_volume,
        days_to_cover,
        short_interest_settlement: short_interest.map(|s| s.settlement_date.clone()),
        amihud_band,
        days_to_cover_band,
        band,
        haircut: band.haircut(),
        gaps,
    }
}

/// The trailing window's liquidity reads.
#[derive(Debug, Default)]
struct LiquidityWindow {
    /// Amihud illiquidity ×10⁶.
    amihud: Option<f64>,
    /// The positive-volume sessions the Amihud read stood on.
    amihud_sessions: usize,
    median_dollar_volume: Option<f64>,
}

/// The Amihud read and the median dollar volume over the trailing
/// [`AMIHUD_SESSIONS`] returns through `as_of`. `Err` is the typed gap where no
/// current bar exists; a window short of [`AMIHUD_MIN_VOLUME_DAYS`] reads each
/// value absent.
fn liquidity_window(bars: &[DailyBar], as_of: &str) -> Result<LiquidityWindow, String> {
    let bars = dedupe_bars(bars);
    let end = bars.partition_point(|b| b.date.as_str() <= as_of);
    let through = &bars[..end];
    let earliest = shift(as_of, PRICE_PROXIMITY_DAYS)
        .ok_or_else(|| format!("tradability unread: the as-of date {as_of} does not parse"))?;
    match through.last() {
        Some(last) if last.date >= earliest => {}
        _ => {
            return Err(format!(
                "Amihud illiquidity unread: no bar within {PRICE_PROXIMITY_DAYS} days of {as_of}"
            ))
        }
    }
    let window = &through[through.len().saturating_sub(AMIHUD_SESSIONS + 1)..];
    let mut ratios = Vec::with_capacity(AMIHUD_SESSIONS);
    let mut dollar_volumes = Vec::with_capacity(AMIHUD_SESSIONS);
    for pair in window.windows(2) {
        let (prev, bar) = (&pair[0], &pair[1]);
        let Some(volume) = bar.volume else { continue };
        let dollar_volume = bar.close * volume;
        if !dollar_volume.is_finite() {
            continue;
        }
        dollar_volumes.push(dollar_volume);
        // A zero-volume session has no price impact to measure; it counts
        // toward the median, never toward the Amihud mean.
        if dollar_volume > 0.0 && prev.close > 0.0 {
            let ratio = (bar.close / prev.close - 1.0).abs() / dollar_volume;
            if ratio.is_finite() {
                ratios.push(ratio);
            }
        }
    }
    let sessions = ratios.len();
    let amihud = (sessions >= AMIHUD_MIN_VOLUME_DAYS)
        .then(|| ratios.iter().sum::<f64>() / sessions as f64 * 1e6)
        .filter(|a| a.is_finite());
    let median_dollar_volume =
        (dollar_volumes.len() >= AMIHUD_MIN_VOLUME_DAYS).then(|| median(&mut dollar_volumes));
    Ok(LiquidityWindow {
        amihud,
        amihud_sessions: sessions,
        median_dollar_volume,
    })
}

/// The bars with each session once, in date order — the last row served for a
/// duplicated date wins (the factor reads' [`super::factors::dedupe_closes`] rule).
fn dedupe_bars(bars: &[DailyBar]) -> Vec<DailyBar> {
    let mut sorted = bars.to_vec();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    let mut out: Vec<DailyBar> = Vec::with_capacity(sorted.len());
    for bar in sorted {
        match out.last_mut() {
            Some(last) if last.date == bar.date => *last = bar,
            _ => out.push(bar),
        }
    }
    out
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` consecutive calendar-day bars ending 2026-10-13, each moving the close
    /// by `step` (alternating sign) on `volume` shares.
    fn bars(n: usize, close: f64, step: f64, volume: Option<f64>) -> Vec<DailyBar> {
        let end = chrono::NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        (0..n)
            .map(|i| {
                let date = end - chrono::Duration::days((n - 1 - i) as i64);
                let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                DailyBar {
                    date: date.format("%Y-%m-%d").to_string(),
                    close: close * (1.0 + sign * step),
                    volume,
                }
            })
            .collect()
    }

    fn short(days_to_cover: Option<f64>) -> ShortInterestRead {
        ShortInterestRead {
            settlement_date: "2026-09-30".into(),
            current_short_interest: 1.0e6,
            previous_short_interest: None,
            average_daily_volume: None,
            days_to_cover,
        }
    }

    const AS_OF: &str = "2026-10-13";

    /// Bars whose Amihud read lands at `target` (×10⁶): a constant 2% absolute
    /// move each session on the dollar volume that prices it there.
    fn bars_at(target: f64) -> Vec<DailyBar> {
        // |r| = 2·step/(1−step) on alternating closes at step 1%: 0.0202…
        let step = 0.01;
        let r = 2.0 * step / (1.0 - step);
        let r_up = 2.0 * step / (1.0 + step);
        let mean_r = (r + r_up) / 2.0;
        // ILLIQ ≈ mean|r| / dollar volume × 1e6, close ~100.
        let dollar_volume = mean_r / target * 1e6;
        bars(80, 100.0, step, Some(dollar_volume / 100.0))
    }

    #[test]
    fn the_amihud_read_is_the_mean_absolute_return_per_million_traded() {
        let read = tradability(&bars(64, 100.0, 0.01, Some(10_000.0)), None, AS_OF);
        // 63 returns alternating 101→99 (−1.98%) and 99→101 (+2.02%), each on
        // its own session's dollar volume.
        let down = (99.0 / 101.0 - 1.0f64).abs() / (99.0 * 10_000.0);
        let up = (101.0 / 99.0 - 1.0f64).abs() / (101.0 * 10_000.0);
        // 63 returns starting from index 1 (a down move): 32 down, 31 up.
        let expected = (32.0 * down + 31.0 * up) / 63.0 * 1e6;
        assert_eq!(read.amihud_sessions, 63);
        assert!((read.amihud.unwrap() - expected).abs() < 1e-9, "{read:?}");
        assert!(read.median_dollar_volume.is_some());
    }

    #[test]
    fn each_amihud_band_edge_flags_at_or_above_it() {
        for (target, band) in [
            (AMIHUD_FLAGGED * 0.9, TradabilityBand::Unflagged),
            (AMIHUD_FLAGGED * 1.1, TradabilityBand::Flagged),
            (AMIHUD_SEVERE * 0.9, TradabilityBand::Flagged),
            (AMIHUD_SEVERE * 1.1, TradabilityBand::Severe),
        ] {
            let read = tradability(&bars_at(target), Some(&short(Some(1.0))), AS_OF);
            assert_eq!(read.amihud_band, Some(band), "{target}: {read:?}");
            assert_eq!(read.band, band, "{target}");
        }
        assert_eq!(
            TradabilityBand::of(AMIHUD_FLAGGED, AMIHUD_FLAGGED, AMIHUD_SEVERE),
            TradabilityBand::Flagged,
            "the edge itself flags"
        );
        assert_eq!(
            TradabilityBand::of(AMIHUD_SEVERE, AMIHUD_FLAGGED, AMIHUD_SEVERE),
            TradabilityBand::Severe
        );
    }

    #[test]
    fn each_days_to_cover_band_edge_flags_at_or_above_it() {
        let liquid = bars_at(AMIHUD_FLAGGED / 10.0);
        for (dtc, band) in [
            (9.99, TradabilityBand::Unflagged),
            (DAYS_TO_COVER_FLAGGED, TradabilityBand::Flagged),
            (19.99, TradabilityBand::Flagged),
            (DAYS_TO_COVER_SEVERE, TradabilityBand::Severe),
        ] {
            let read = tradability(&liquid, Some(&short(Some(dtc))), AS_OF);
            assert_eq!(read.days_to_cover_band, Some(band), "{dtc}");
            assert_eq!(read.band, band, "{dtc}");
            assert_eq!(read.short_interest_settlement.as_deref(), Some("2026-09-30"));
        }
    }

    #[test]
    fn the_worse_leg_sets_the_band_and_the_band_sets_the_haircut() {
        let read = tradability(
            &bars_at(AMIHUD_SEVERE * 2.0),
            Some(&short(Some(DAYS_TO_COVER_FLAGGED))),
            AS_OF,
        );
        assert_eq!(read.amihud_band, Some(TradabilityBand::Severe));
        assert_eq!(read.days_to_cover_band, Some(TradabilityBand::Flagged));
        assert_eq!(read.band, TradabilityBand::Severe);
        assert_eq!(read.haircut, 0.06);
        assert_eq!(TradabilityBand::Unflagged.haircut(), 0.0);
        assert_eq!(TradabilityBand::Flagged.haircut(), 0.03);
        assert_eq!(TradabilityBand::Severe.haircut(), 0.06);
        assert!(read.gaps.is_empty(), "{:?}", read.gaps);
    }

    #[test]
    fn a_missing_leg_cannot_trigger_and_both_missing_reads_unflagged_with_a_gap() {
        // Only days-to-cover read: it alone sets the band.
        let read = tradability(&[], Some(&short(Some(DAYS_TO_COVER_SEVERE))), AS_OF);
        assert_eq!(read.amihud, None);
        assert_eq!(read.band, TradabilityBand::Severe);
        assert!(read.gaps.iter().any(|g| g.contains("no bar within")), "{:?}", read.gaps);
        // Only Amihud read: a FINRA row without the figure is a gap, not a flag.
        let read = tradability(&bars_at(AMIHUD_FLAGGED * 2.0), Some(&short(None)), AS_OF);
        assert_eq!(read.band, TradabilityBand::Flagged);
        assert!(read.gaps.iter().any(|g| g.contains("served none")), "{:?}", read.gaps);
        // Neither: unflagged, never a fabricated flag, with the gap logged.
        let read = tradability(&[], None, AS_OF);
        assert_eq!(read.band, TradabilityBand::Unflagged);
        assert_eq!(read.haircut, 0.0);
        assert!(
            read.gaps.iter().any(|g| g == "no tradability input read — unflagged"),
            "{:?}",
            read.gaps
        );
    }

    #[test]
    fn the_amihud_read_needs_fifty_positive_volume_sessions() {
        // 64 bars, 14 of them zero-volume and 1 with no served volume: 48
        // positive-volume returns — below the floor.
        let mut series = bars(64, 100.0, 0.01, Some(10_000.0));
        for bar in series.iter_mut().skip(1).take(14) {
            bar.volume = Some(0.0);
        }
        series[20].volume = None;
        let read = tradability(&series, None, AS_OF);
        assert_eq!(read.amihud_sessions, 48);
        assert_eq!(read.amihud, None);
        assert!(
            read.gaps.iter().any(|g| g.contains("48 positive-volume session(s)")),
            "{:?}",
            read.gaps
        );
        // The zero-volume sessions still count toward the median's floor
        // (62 served volumes), so the median reads.
        assert!(read.median_dollar_volume.is_some());
        // One more positive-volume session clears the floor.
        series[20].volume = Some(10_000.0);
        series[1].volume = Some(10_000.0);
        let read = tradability(&series, None, AS_OF);
        assert_eq!(read.amihud_sessions, 50);
        assert!(read.amihud.is_some());
    }

    #[test]
    fn a_stale_series_or_a_duplicated_session_never_stands_in() {
        // The newest bar 11 days before the as-of date: no current read.
        let read = tradability(&bars(80, 100.0, 0.01, Some(1.0)), None, "2026-10-24");
        assert_eq!(read.amihud, None);
        assert!(read.gaps.iter().any(|g| g.contains("no bar within 10 days")));
        // A session served twice counts once, the last row winning.
        let mut series = bars(64, 100.0, 0.01, Some(10_000.0));
        let mut dup = series[63].clone();
        dup.volume = Some(20_000.0);
        series.reverse();
        series.push(dup);
        let read = tradability(&series, None, AS_OF);
        assert_eq!(read.amihud_sessions, 63);
        let clean = tradability(&bars(64, 100.0, 0.01, Some(10_000.0)), None, AS_OF);
        assert!(read.amihud.unwrap() < clean.amihud.unwrap(), "the doubled last volume wins");
    }

    #[test]
    fn the_median_dollar_volume_is_close_times_volume() {
        let read = tradability(&bars(64, 100.0, 0.0, Some(5_000.0)), None, AS_OF);
        assert_eq!(read.median_dollar_volume, Some(500_000.0));
        // A flat price has no impact to measure: Amihud reads zero, unflagged.
        assert_eq!(read.amihud, Some(0.0));
        assert_eq!(read.amihud_band, Some(TradabilityBand::Unflagged));
    }
}
