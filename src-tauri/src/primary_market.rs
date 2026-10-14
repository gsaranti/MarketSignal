//! The primary-market pace (`docs/data-sources.md §Report enrichment`): FMP's paid
//! `ipos-calendar` and `mergers-acquisitions-latest` reduced to IPO and deal counts in
//! the recent window against the equal-length window before it, the IPOs scheduled
//! ahead, and the recent window's notable names. The wire fetch, the row shapers and
//! the walk live in `fmp`. A count is a completeness claim, so a side whose feed served
//! a row no reader could place is withheld, never undercounted.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, NaiveDate};

use crate::data_sources::{
    ActivityWindow, DealActivity, GapReason, IpoActivity, NotableDeal, NotableIpo,
    PrimaryMarketActivity,
};
use crate::portfolio::evidence::{MaDeal, MaWalk, MaWalkEnd};

/// The upcoming-IPO window, in days from the session date, the session date included.
pub const UPCOMING_DAYS: i64 = 14;

/// The most notable names each side carries (drafted).
pub const NOTABLE_CAP: usize = 5;

/// The window a date falls in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Prior,
    Recent,
    Upcoming,
}

/// The three half-open windows on the ET session date T: `prior` [T−2N, T−N), `recent`
/// [T−N, T) and `upcoming` [T, T + [`UPCOMING_DAYS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrothWindows {
    prior_start: NaiveDate,
    recent_start: NaiveDate,
    today: NaiveDate,
    upcoming_end: NaiveDate,
}

impl FrothWindows {
    /// The windows for the session date `today` and a recent window of `window_days`.
    pub fn new(today: NaiveDate, window_days: i64) -> Self {
        let n = Duration::days(window_days);
        Self {
            prior_start: today - n - n,
            recent_start: today - n,
            today,
            upcoming_end: today + Duration::days(UPCOMING_DAYS),
        }
    }

    /// The window `date` (`YYYY-MM-DD`) falls in; `None` outside all three or unparseable.
    pub fn window_of(&self, date: &str) -> Option<Window> {
        let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        if d < self.prior_start || d >= self.upcoming_end {
            None
        } else if d < self.recent_start {
            Some(Window::Prior)
        } else if d < self.today {
            Some(Window::Recent)
        } else {
            Some(Window::Upcoming)
        }
    }

    /// The first day any window covers: the IPO fetch's `from` and the M&A walk's start.
    pub fn first_day(&self) -> NaiveDate {
        self.prior_start
    }

    /// The last day any window covers: the IPO fetch's inclusive `to`.
    pub fn last_day(&self) -> NaiveDate {
        self.upcoming_end - Duration::days(1)
    }

    fn served(from: NaiveDate, end: NaiveDate) -> ActivityWindow {
        ActivityWindow {
            from: from.format("%Y-%m-%d").to_string(),
            to: (end - Duration::days(1)).format("%Y-%m-%d").to_string(),
        }
    }
}

/// An IPO calendar row's status as served (`actions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpoStatus {
    Priced,
    Expected,
    Other,
}

impl IpoStatus {
    /// The status a served `actions` value names; anything but the two the feed serves is
    /// `Other`, never counted.
    pub fn from_served(actions: Option<&str>) -> Self {
        match actions.map(str::trim) {
            Some(a) if a.eq_ignore_ascii_case("priced") => IpoStatus::Priced,
            Some(a) if a.eq_ignore_ascii_case("expected") => IpoStatus::Expected,
            _ => IpoStatus::Other,
        }
    }
}

/// One readable `ipos-calendar` row.
#[derive(Debug, Clone, PartialEq)]
pub struct IpoRow {
    /// The listing's symbol; a row without one counts alone, outside the one-per-symbol rule.
    pub symbol: Option<String>,
    pub company: Option<String>,
    /// The listing date, `YYYY-MM-DD`.
    pub date: String,
    pub exchange: Option<String>,
    pub status: IpoStatus,
    /// The shares offered; a row without them is not an offering this read counts.
    pub shares: Option<f64>,
    /// The offering's dollar size as served (`dealSize`).
    pub deal_size: Option<f64>,
}

impl IpoRow {
    fn is_offering(&self) -> bool {
        self.shares.is_some_and(|s| s > 0.0)
    }
}

/// The IPO side over the calendar's readable rows, with the gap the manifest records
/// (`docs/data-sources.md §Report enrichment`). `undated_offerings` counts served rows
/// with a share count but no readable date: one withholds the read with a `Malformed`
/// gap. No row in any window is a fault of the feed, never a freeze: `Unavailable`.
pub fn ipo_activity(
    rows: &[IpoRow],
    undated_offerings: usize,
    windows: &FrothWindows,
) -> (Option<IpoActivity>, Option<GapReason>) {
    if undated_offerings > 0 {
        return (None, Some(GapReason::Malformed));
    }
    let mut by_symbol: BTreeMap<&str, &IpoRow> = BTreeMap::new();
    let mut symbolless: Vec<&IpoRow> = Vec::new();
    for row in rows {
        if windows.window_of(&row.date).is_none() {
            continue;
        }
        let Some(symbol) = row.symbol.as_deref() else {
            symbolless.push(row);
            continue;
        };
        by_symbol
            .entry(symbol)
            .and_modify(|kept| {
                if row.date > kept.date {
                    *kept = row;
                }
            })
            .or_insert(row);
    }
    if by_symbol.is_empty() && symbolless.is_empty() {
        return (None, Some(GapReason::Unavailable));
    }
    let (mut recent_count, mut prior_count, mut upcoming_count) = (0u32, 0u32, 0u32);
    let mut notable: Vec<&IpoRow> = Vec::new();
    let kept = by_symbol.values().copied().chain(symbolless);
    for row in kept.filter(|r| r.is_offering()) {
        match (windows.window_of(&row.date), row.status) {
            (Some(Window::Recent), IpoStatus::Priced) => {
                recent_count += 1;
                notable.push(row);
            }
            (Some(Window::Prior), IpoStatus::Priced) => prior_count += 1,
            (Some(Window::Upcoming), IpoStatus::Expected) => upcoming_count += 1,
            _ => {}
        }
    }
    notable.sort_by(|a, b| by_size_then_date(a, b));
    let activity = IpoActivity {
        recent_count,
        prior_count,
        upcoming_count,
        notable: notable
            .into_iter()
            .take(NOTABLE_CAP)
            .map(|r| NotableIpo {
                symbol: r.symbol.clone(),
                company: r.company.clone(),
                date: r.date.clone(),
                exchange: r.exchange.clone(),
                deal_size_usd: r.deal_size,
            })
            .collect(),
    };
    (Some(activity), None)
}

/// Largest `deal_size` first, a sizeless row after every sized one; then newest first,
/// then by symbol so the order is total.
fn by_size_then_date(a: &IpoRow, b: &IpoRow) -> Ordering {
    let size = match (a.deal_size, b.deal_size) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    size.then_with(|| b.date.cmp(&a.date))
        .then_with(|| a.symbol.cmp(&b.symbol))
}

/// The deal side over the walk, with the gap the manifest records
/// (`docs/data-sources.md §Report enrichment`): one deal per filing, sitting in the
/// window of its earliest walked row; an unreadable row withholds the read with a
/// `Malformed` gap; an incomplete walk reads the recent count alone once a row in the
/// prior window was seen, else no read and the stopping reason as the gap. A first-page
/// `OutOfScope` and a cancel record none.
pub fn deal_activity(
    walk: &MaWalk,
    windows: &FrothWindows,
) -> (Option<DealActivity>, Option<GapReason>) {
    let end = &walk.end;
    let complete = match end {
        MaWalkEnd::Boundary | MaWalkEnd::FeedEnd => true,
        MaWalkEnd::Cancelled => return (None, None),
        MaWalkEnd::EmptyFeed => return (None, Some(GapReason::Unavailable)),
        MaWalkEnd::Failed {
            page: 0, reason, ..
        } => {
            let gap = (*reason != GapReason::OutOfScope).then_some(*reason);
            return (None, gap);
        }
        MaWalkEnd::Cap | MaWalkEnd::Failed { .. } => false,
    };
    if walk.unreadable > 0 {
        return (None, Some(GapReason::Malformed));
    }
    let unique = unique_deals(&walk.deals);
    let in_window = |w: Window| {
        unique
            .iter()
            .filter(move |d| windows.window_of(&d.transaction_date) == Some(w))
    };
    let prior_count = in_window(Window::Prior).count() as u32;
    if !complete && prior_count == 0 {
        let reason = match end {
            MaWalkEnd::Failed { reason, .. } => *reason,
            _ => GapReason::Unavailable,
        };
        return (None, Some(reason));
    }
    let mut recent: Vec<&MaDeal> = in_window(Window::Recent).copied().collect();
    // Newest first; the sort is stable, so one date keeps the served order.
    recent.sort_by(|a, b| b.transaction_date.cmp(&a.transaction_date));
    let activity = DealActivity {
        recent_count: recent.len() as u32,
        prior_count: complete.then_some(prior_count),
        notable: recent
            .into_iter()
            .take(NOTABLE_CAP)
            .map(|d| NotableDeal {
                acquirer: d.acquirer_name.clone().or_else(|| d.acquirer_symbol.clone()),
                target: d.target_name.clone().or_else(|| d.target_symbol.clone()),
                target_symbol: d.target_symbol.clone(),
                date: d.transaction_date.clone(),
            })
            .collect(),
    };
    (Some(activity), None)
}

/// One deal per registration filing: the feed serves a row per acquirer security (share
/// classes, warrants, units, preferreds), every one carrying the filing's `link`; a row
/// with no link falls back to its (acquirer, target) names, case-folded, and a row with
/// neither counts as a deal of its own. The earliest row of a deal wins, a tie keeping the
/// first served.
fn unique_deals(deals: &[MaDeal]) -> Vec<&MaDeal> {
    let mut out: Vec<&MaDeal> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for deal in deals {
        let Some(key) = deal_key(deal) else {
            out.push(deal);
            continue;
        };
        match index.get(&key) {
            Some(&i) => {
                if deal.transaction_date < out[i].transaction_date {
                    out[i] = deal;
                }
            }
            None => {
                index.insert(key, out.len());
                out.push(deal);
            }
        }
    }
    out
}

/// The identity a deal's rows share — its filing `link`, else its party names (each
/// party's symbol where it has no name) — or `None` for a row naming neither.
fn deal_key(deal: &MaDeal) -> Option<String> {
    let trimmed = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
    };
    if let Some(link) = trimmed(&deal.link) {
        return Some(format!("link:{link}"));
    }
    let acquirer = trimmed(&deal.acquirer_name).or_else(|| trimmed(&deal.acquirer_symbol));
    let target = trimmed(&deal.target_name).or_else(|| trimmed(&deal.target_symbol));
    if acquirer.is_none() && target.is_none() {
        return None;
    }
    Some(format!(
        "names:{}|{}",
        acquirer.unwrap_or_default(),
        target.unwrap_or_default()
    ))
}

/// The group: the three windows with whichever sides were read, or `None` when neither
/// was — a failed gather leaves no primary-market signal.
pub fn activity(
    windows: &FrothWindows,
    ipos: Option<IpoActivity>,
    deals: Option<DealActivity>,
) -> Option<PrimaryMarketActivity> {
    if ipos.is_none() && deals.is_none() {
        return None;
    }
    Some(PrimaryMarketActivity {
        recent: FrothWindows::served(windows.recent_start, windows.today),
        prior: FrothWindows::served(windows.prior_start, windows.recent_start),
        upcoming: FrothWindows::served(windows.today, windows.upcoming_end),
        ipos,
        deals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// N = 30 on 2026-10-12: prior 08-13..09-11, recent 09-12..10-11, upcoming 10-12..10-25.
    fn windows() -> FrothWindows {
        FrothWindows::new(d("2026-10-12"), 30)
    }

    /// The IPO read over rows holding no undated offering.
    fn ipos_over(rows: &[IpoRow]) -> (Option<IpoActivity>, Option<GapReason>) {
        ipo_activity(rows, 0, &windows())
    }

    /// The deal read over a walk that read every row it was served.
    fn deals_over(deals: &[MaDeal], end: &MaWalkEnd) -> (Option<DealActivity>, Option<GapReason>) {
        let walk = MaWalk {
            deals: deals.to_vec(),
            unreadable: 0,
            end: end.clone(),
        };
        deal_activity(&walk, &windows())
    }

    fn ipo(symbol: &str, date: &str, status: IpoStatus, shares: Option<f64>) -> IpoRow {
        IpoRow {
            symbol: Some(symbol.into()),
            company: Some(format!("{symbol} Inc.")),
            date: date.into(),
            exchange: Some("NASDAQ".into()),
            status,
            shares,
            deal_size: None,
        }
    }

    fn sized(mut row: IpoRow, size: f64) -> IpoRow {
        row.deal_size = Some(size);
        row
    }

    fn deal(acquirer: &str, target: &str, date: &str, link: Option<&str>) -> MaDeal {
        MaDeal {
            acquirer_symbol: Some(acquirer.to_uppercase()),
            acquirer_name: Some(acquirer.into()),
            target_symbol: Some(target.to_uppercase()),
            target_name: Some(target.into()),
            transaction_date: date.into(),
            link: link.map(String::from),
        }
    }

    #[test]
    fn windows_are_half_open_on_the_session_date() {
        let w = windows();
        assert_eq!(w.window_of("2026-08-12"), None, "a day before T−2N");
        assert_eq!(w.window_of("2026-08-13"), Some(Window::Prior), "T−2N opens prior");
        assert_eq!(w.window_of("2026-09-11"), Some(Window::Prior));
        assert_eq!(w.window_of("2026-09-12"), Some(Window::Recent), "T−N opens recent");
        assert_eq!(w.window_of("2026-10-11"), Some(Window::Recent));
        assert_eq!(w.window_of("2026-10-12"), Some(Window::Upcoming), "T opens upcoming");
        assert_eq!(w.window_of("2026-10-25"), Some(Window::Upcoming));
        assert_eq!(w.window_of("2026-10-26"), None, "T+14 is past upcoming");
        assert_eq!(w.window_of("not a date"), None);
        assert_eq!(w.first_day(), d("2026-08-13"));
        assert_eq!(w.last_day(), d("2026-10-25"));
    }

    #[test]
    fn windows_serialize_as_first_and_last_days() {
        let g = activity(&windows(), None, Some(empty_deals())).unwrap();
        assert_eq!(
            (g.prior.from.as_str(), g.prior.to.as_str()),
            ("2026-08-13", "2026-09-11")
        );
        assert_eq!(
            (g.recent.from.as_str(), g.recent.to.as_str()),
            ("2026-09-12", "2026-10-11")
        );
        assert_eq!(
            (g.upcoming.from.as_str(), g.upcoming.to.as_str()),
            ("2026-10-12", "2026-10-25")
        );
        // A one-week cadence: each window seven days long.
        let week = activity(&FrothWindows::new(d("2026-10-12"), 7), None, Some(empty_deals()))
            .unwrap();
        assert_eq!(
            (week.prior.from.as_str(), week.prior.to.as_str()),
            ("2026-09-28", "2026-10-04")
        );
        assert_eq!(
            (week.recent.from.as_str(), week.recent.to.as_str()),
            ("2026-10-05", "2026-10-11")
        );
    }

    fn empty_deals() -> DealActivity {
        DealActivity {
            recent_count: 0,
            prior_count: Some(0),
            notable: Vec::new(),
        }
    }

    #[test]
    fn no_side_read_leaves_no_group() {
        assert_eq!(activity(&windows(), None, None), None);
    }

    #[test]
    fn ipos_count_offerings_with_a_share_count_by_status_and_window() {
        let rows = vec![
            ipo("AAA", "2026-10-09", IpoStatus::Priced, Some(8.0e6)),
            ipo("BBB", "2026-09-12", IpoStatus::Priced, Some(3.0e6)),
            // Recent but expected: neither priced nor upcoming.
            ipo("CCC", "2026-09-30", IpoStatus::Expected, Some(5.0e7)),
            // No share count: a fund launch, a note or a warrant line.
            ipo("ETFX", "2026-10-07", IpoStatus::Priced, None),
            ipo("WRNT", "2026-10-07", IpoStatus::Priced, Some(0.0)),
            ipo("PPP", "2026-08-13", IpoStatus::Priced, Some(1.0e6)),
            ipo("QQQ", "2026-09-11", IpoStatus::Priced, Some(1.0e6)),
            ipo("UUU", "2026-10-12", IpoStatus::Expected, Some(9.0e6)),
            ipo("VVV", "2026-10-25", IpoStatus::Expected, Some(9.0e6)),
            // Upcoming but priced: not a scheduled offering.
            ipo("WWW", "2026-10-13", IpoStatus::Priced, Some(9.0e6)),
            ipo("XXX", "2026-10-14", IpoStatus::Other, Some(9.0e6)),
            // Outside every window.
            ipo("OLD", "2026-08-12", IpoStatus::Priced, Some(1.0e6)),
            ipo("FAR", "2026-10-26", IpoStatus::Expected, Some(1.0e6)),
        ];
        let read = ipos_over(&rows).0.unwrap();
        assert_eq!(
            (read.recent_count, read.prior_count, read.upcoming_count),
            (2, 2, 2)
        );
        let names: Vec<&str> = read.notable.iter().map(|n| n.symbol.as_deref().unwrap()).collect();
        assert_eq!(names, vec!["AAA", "BBB"], "sizeless, so newest first");
    }

    #[test]
    fn a_symbol_counts_once_on_its_latest_dated_row() {
        let rows = vec![
            ipo("AAA", "2026-09-01", IpoStatus::Expected, Some(1.0e6)),
            ipo("AAA", "2026-09-20", IpoStatus::Priced, Some(1.0e6)),
        ];
        let read = ipos_over(&rows).0.unwrap();
        assert_eq!((read.recent_count, read.prior_count), (1, 0));
    }

    #[test]
    fn an_ipo_calendar_with_no_row_in_any_window_is_not_a_read() {
        assert_eq!(ipos_over(&[]), (None, Some(GapReason::Unavailable)));
        let outside = vec![ipo("OLD", "2026-07-01", IpoStatus::Priced, Some(1.0e6))];
        assert_eq!(ipos_over(&outside), (None, Some(GapReason::Unavailable)));
        // A window holding only rows without a share count reads as zero offerings.
        let funds = vec![ipo("ETFX", "2026-10-07", IpoStatus::Priced, None)];
        let read = ipos_over(&funds).0.unwrap();
        assert_eq!(
            (read.recent_count, read.prior_count, read.upcoming_count),
            (0, 0, 0)
        );
        assert!(read.notable.is_empty());
    }

    #[test]
    fn notable_ipos_rank_by_deal_size_then_date_and_cap_at_five() {
        let rows = vec![
            sized(ipo("S1", "2026-09-20", IpoStatus::Priced, Some(1.0)), 100.0),
            sized(ipo("S2", "2026-09-21", IpoStatus::Priced, Some(1.0)), 540.0),
            ipo("N1", "2026-10-01", IpoStatus::Priced, Some(1.0)),
            ipo("N2", "2026-10-09", IpoStatus::Priced, Some(1.0)),
            sized(ipo("S3", "2026-09-22", IpoStatus::Priced, Some(1.0)), 250.0),
            sized(ipo("S4", "2026-09-23", IpoStatus::Priced, Some(1.0)), 250.0),
            // Sized but in the prior window: counted, never notable.
            sized(ipo("P1", "2026-09-01", IpoStatus::Priced, Some(1.0)), 9_999.0),
        ];
        let read = ipos_over(&rows).0.unwrap();
        assert_eq!(read.recent_count, 6);
        let names: Vec<&str> = read.notable.iter().map(|n| n.symbol.as_deref().unwrap()).collect();
        assert_eq!(names, vec!["S2", "S4", "S3", "S1", "N2"]);
        assert_eq!(read.notable[0].deal_size_usd, Some(540.0));
        assert_eq!(read.notable[4].deal_size_usd, None);
        assert_eq!(read.notable[0].company.as_deref(), Some("S2 Inc."));
        assert_eq!(read.notable[0].exchange.as_deref(), Some("NASDAQ"));
    }

    #[test]
    fn a_row_without_a_symbol_counts_alone() {
        let mut first = ipo("X", "2026-10-01", IpoStatus::Priced, Some(1.0e6));
        first.symbol = None;
        let mut second = ipo("Y", "2026-10-02", IpoStatus::Priced, Some(1.0e6));
        second.symbol = None;
        let read = ipos_over(&[first, second]).0.unwrap();
        assert_eq!(read.recent_count, 2);
        assert!(read.notable.iter().all(|n| n.symbol.is_none()));
    }

    #[test]
    fn an_undated_offering_withholds_the_ipo_read() {
        // A served offering the reader could not date might belong to any window, so the
        // counts beside it are not complete.
        let rows = vec![ipo("PPP", "2026-09-01", IpoStatus::Priced, Some(1.0e6))];
        assert_eq!(
            ipo_activity(&rows, 1, &windows()),
            (None, Some(GapReason::Malformed))
        );
        assert!(ipo_activity(&rows, 0, &windows()).0.is_some());
    }

    #[test]
    fn ipo_status_reads_the_two_served_values() {
        assert_eq!(IpoStatus::from_served(Some("Priced")), IpoStatus::Priced);
        assert_eq!(IpoStatus::from_served(Some(" expected ")), IpoStatus::Expected);
        assert_eq!(IpoStatus::from_served(Some("Withdrawn")), IpoStatus::Other);
        assert_eq!(IpoStatus::from_served(None), IpoStatus::Other);
    }

    /// Newest first, as the feed serves it: two filings in the recent window (one fanned out
    /// over three securities and two acquirer names), one in the prior window.
    fn walked() -> Vec<MaDeal> {
        vec![
            deal("Inflection Point VII", "Intuitive Machines", "2026-10-08", Some("L1")),
            deal("Inflection Point VII*", "Intuitive Machines", "2026-10-08", Some("L1")),
            deal("Inflection Point VII", "Intuitive Machines", "2026-10-08", Some("L1")),
            deal("HomeTrust", "Blue Ridge", "2026-10-02", Some("L2")),
            deal("Overstock", "Fathom", "2026-08-17", Some("L3")),
            deal("Beyond", "Fathom", "2026-08-17", Some("L3")),
        ]
    }

    #[test]
    fn a_complete_walk_counts_one_deal_per_filing_in_each_window() {
        for end in [MaWalkEnd::Boundary, MaWalkEnd::FeedEnd] {
            let (read, gap) = deals_over(&walked(), &end);
            let read = read.unwrap();
            assert_eq!(gap, None);
            assert_eq!((read.recent_count, read.prior_count), (2, Some(1)));
            let targets: Vec<Option<&str>> =
                read.notable.iter().map(|n| n.target.as_deref()).collect();
            assert_eq!(targets, vec![Some("Intuitive Machines"), Some("Blue Ridge")]);
            assert_eq!(read.notable[0].acquirer.as_deref(), Some("Inflection Point VII"));
            assert_eq!(read.notable[0].target_symbol.as_deref(), Some("INTUITIVE MACHINES"));
            assert_eq!(read.notable[0].date, "2026-10-08");
        }
    }

    #[test]
    fn a_row_without_a_link_falls_back_to_its_names() {
        let rows = vec![
            deal("Fox Corp", "Roku", "2026-10-01", None),
            deal("FOX CORP", " roku ", "2026-09-30", Some("  ")),
            deal("Fox Corp", "Tubi", "2026-09-29", None),
        ];
        let (read, _) = deals_over(&rows, &MaWalkEnd::Boundary);
        let read = read.unwrap();
        assert_eq!(read.recent_count, 2, "the name pair joins the first two rows");
        // The joined deal keeps its earliest row; Tubi is its own deal.
        assert_eq!(read.notable[0].date, "2026-09-30");
        assert_eq!(read.notable[1].target.as_deref(), Some("Tubi"));
    }

    #[test]
    fn a_row_naming_no_party_counts_as_a_deal_of_its_own() {
        let anonymous = |date: &str| MaDeal {
            transaction_date: date.into(),
            ..Default::default()
        };
        let rows = vec![anonymous("2026-10-01"), anonymous("2026-10-01")];
        let (read, _) = deals_over(&rows, &MaWalkEnd::Boundary);
        assert_eq!(read.unwrap().recent_count, 2);
    }

    #[test]
    fn a_deal_whose_earliest_walked_row_predates_the_windows_counts_nowhere() {
        // The boundary page's rows before the windows ride the dedup, so a later row of the
        // same deal never places it in a window.
        let rows = vec![
            deal("A", "B", "2026-09-15", Some("L9")),
            deal("A", "B", "2026-08-12", Some("L9")),
        ];
        let (read, gap) = deals_over(&rows, &MaWalkEnd::Boundary);
        assert_eq!(gap, None);
        let read = read.unwrap();
        assert_eq!((read.recent_count, read.prior_count), (0, Some(0)));
        assert!(read.notable.is_empty());
    }

    #[test]
    fn an_unreadable_walked_row_withholds_the_deal_read() {
        let walk = MaWalk {
            deals: walked(),
            unreadable: 1,
            end: MaWalkEnd::Boundary,
        };
        assert_eq!(
            deal_activity(&walk, &windows()),
            (None, Some(GapReason::Malformed))
        );
    }

    #[test]
    fn a_deal_sits_in_the_window_of_its_earliest_row() {
        let rows = vec![
            deal("A", "B", "2026-09-15", Some("L9")),
            deal("A", "B", "2026-09-05", Some("L9")),
        ];
        let (read, _) = deals_over(&rows, &MaWalkEnd::Boundary);
        let read = read.unwrap();
        assert_eq!((read.recent_count, read.prior_count), (0, Some(1)));
    }

    #[test]
    fn a_walk_stopped_after_the_recent_window_reads_the_recent_count_alone() {
        let failed = MaWalkEnd::Failed {
            page: 3,
            reason: GapReason::Unavailable,
            detail: "unavailable".into(),
        };
        for end in [MaWalkEnd::Cap, failed] {
            let (read, gap) = deals_over(&walked(), &end);
            let read = read.unwrap();
            assert_eq!(gap, None);
            assert_eq!((read.recent_count, read.prior_count), (2, None));
        }
    }

    #[test]
    fn a_walk_stopped_inside_the_recent_window_leaves_no_read_and_one_gap() {
        let recent_only: Vec<MaDeal> = walked().into_iter().take(4).collect();
        let (read, gap) = deals_over(&recent_only, &MaWalkEnd::Cap);
        assert_eq!((read, gap), (None, Some(GapReason::Unavailable)));
        let failed = MaWalkEnd::Failed {
            page: 2,
            reason: GapReason::Malformed,
            detail: "no served row was readable".into(),
        };
        let (read, gap) = deals_over(&recent_only, &failed);
        assert_eq!((read, gap), (None, Some(GapReason::Malformed)));
    }

    #[test]
    fn a_first_page_failure_or_an_empty_feed_leaves_no_read() {
        let first = |reason| MaWalkEnd::Failed {
            page: 0,
            reason,
            detail: String::new(),
        };
        assert_eq!(
            deals_over(&[], &first(GapReason::Rejected)),
            (None, Some(GapReason::Rejected))
        );
        assert_eq!(
            deals_over(&[], &first(GapReason::OutOfScope)),
            (None, None),
            "a key below the paid tier is silent"
        );
        assert_eq!(
            deals_over(&[], &MaWalkEnd::EmptyFeed),
            (None, Some(GapReason::Unavailable))
        );
        assert_eq!(deals_over(&walked(), &MaWalkEnd::Cancelled), (None, None));
    }

    #[test]
    fn a_complete_walk_with_no_deal_in_the_windows_reads_zero() {
        let old = vec![deal("A", "B", "2026-05-14", Some("L"))];
        let (read, gap) = deals_over(&old, &MaWalkEnd::Boundary);
        assert_eq!(gap, None);
        let read = read.unwrap();
        assert_eq!((read.recent_count, read.prior_count), (0, Some(0)));
        assert!(read.notable.is_empty());
    }

    #[test]
    fn notable_deals_are_the_five_newest_recent_deals() {
        let rows: Vec<MaDeal> = (1..=7)
            .rev()
            .map(|i| {
                deal(
                    &format!("Acq{i}"),
                    &format!("Tgt{i}"),
                    &format!("2026-10-0{i}"),
                    Some(&format!("L{i}")),
                )
            })
            .collect();
        let (read, _) = deals_over(&rows, &MaWalkEnd::Boundary);
        let read = read.unwrap();
        assert_eq!(read.recent_count, 7);
        let dates: Vec<&str> = read.notable.iter().map(|n| n.date.as_str()).collect();
        assert_eq!(
            dates,
            vec!["2026-10-07", "2026-10-06", "2026-10-05", "2026-10-04", "2026-10-03"]
        );
    }

    #[test]
    fn a_nameless_party_falls_back_to_its_symbol() {
        let mut row = deal("A", "B", "2026-10-01", Some("L"));
        row.acquirer_name = None;
        row.target_name = None;
        let (read, _) = deals_over(&[row], &MaWalkEnd::Boundary);
        let n = &read.unwrap().notable[0];
        assert_eq!((n.acquirer.as_deref(), n.target.as_deref()), (Some("A"), Some("B")));
    }
}
