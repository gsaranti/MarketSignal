//! Report cadence: how long it has been since the previous Market Signal report,
//! and the qualitative bucket that decision drives.
//!
//! Market Signal runs **on demand** — there is no scheduler (`docs/scheduling.md
//! §Generating a Report`), so the interval between two reports is whatever the user
//! chooses: intraday, daily, weekly, monthly, or an irregular gap. Two halves of the
//! pipeline need to treat that interval as a first-class input rather than assuming a
//! fixed week:
//!
//! - **Data collection** sizes its lookback windows to the elapsed interval (the
//!   GDELT timespan and the Tavily recency window — see `gdelt`/`tavily`), so a daily
//!   run isn't fed a week of stale news and a monthly run isn't starved of it.
//! - **The agents** are told the cadence so they write differently for a tight daily
//!   update versus a full monthly structural refresh (`model_agent`/`analyst_agent`).
//!
//! The elapsed interval is computed once in the application layer
//! (`pipeline::compute_cadence`) from the previous report's snapshot timestamp
//! (`as_of − prior.captured_at`) and threaded to those consumers; this type is a thin,
//! pure classification over that number ([`ReportCadence::from_elapsed`]). It is
//! computed *independently of the change view* (`baseline_delta`): both read the same
//! prior snapshot, but cadence needs only its `captured_at`, so a prior whose baseline
//! payload won't decode still yields the true cadence even though the change view
//! degrades to nothing. `None` elapsed is the first report (no prior snapshot to
//! measure against), which classifies as the first-report cadence.

/// Below this many days elapsed, the run is treated as *intraday* — a same-day
/// regeneration. Above [`MONTHLY_MAX_DAYS`] it is *quarterly+* (a long gap). The
/// boundaries between are app-layer tunables, not doc-pinned (the docs fix no cadence
/// vocabulary); they join the pipeline's other calibrated thresholds.
const INTRADAY_MAX_DAYS: f64 = 1.0;
const DAILY_MAX_DAYS: f64 = 3.0;
const WEEKLY_MAX_DAYS: f64 = 14.0;
const MONTHLY_MAX_DAYS: f64 = 55.0;

/// The qualitative cadence bucket, derived from the elapsed interval. Each bucket
/// names how the run should be approached — the prose lives in
/// [`ReportCadence::main_agent_guidance`] / [`ReportCadence::analyst_cue`], the data
/// windows in the adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadenceBucket {
    /// No prior report — the first run. Nothing to build on or measure against.
    FirstReport,
    /// Less than a day since the previous report.
    Intraday,
    /// Roughly a day.
    Daily,
    /// A few days to ~two weeks — the standard interval.
    Weekly,
    /// Two weeks to ~two months.
    Monthly,
    /// A long gap — over ~two months since the previous report.
    Quarterly,
}

/// How long since the previous report, classified. Construct from the canonical
/// elapsed interval via [`ReportCadence::from_elapsed`] (`None` on the first report).
/// `Copy` so it threads through the news-gather and agent calls by value; `Default`
/// is the first-report cadence (no prior interval), which is what an unset
/// `MainAgentInput.cadence` should mean.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReportCadence {
    elapsed_days: Option<f64>,
}

impl ReportCadence {
    /// Build the cadence from the change view's elapsed interval. `None` is the first
    /// report (no prior snapshot, so no interval to measure). A negative value (clock
    /// skew between runs) is floored to the first non-zero bucket by [`Self::bucket`].
    pub fn from_elapsed(elapsed_days: Option<f64>) -> Self {
        Self { elapsed_days }
    }

    /// The raw elapsed interval in days, or `None` on the first report. The data
    /// adapters read this to size their lookback windows.
    pub fn elapsed_days(&self) -> Option<f64> {
        self.elapsed_days
    }

    /// Classify the elapsed interval into a [`CadenceBucket`]. The bands are
    /// half-open `[lo, hi)` so each boundary value lands in the longer bucket.
    pub fn bucket(&self) -> CadenceBucket {
        match self.elapsed_days {
            None => CadenceBucket::FirstReport,
            Some(d) if d < INTRADAY_MAX_DAYS => CadenceBucket::Intraday,
            Some(d) if d < DAILY_MAX_DAYS => CadenceBucket::Daily,
            Some(d) if d < WEEKLY_MAX_DAYS => CadenceBucket::Weekly,
            Some(d) if d < MONTHLY_MAX_DAYS => CadenceBucket::Monthly,
            Some(_) => CadenceBucket::Quarterly,
        }
    }

    /// The cadence-posture block appended to the main agent's prompt: it tells the
    /// Head Market Analyst how to pitch *this* report given the interval — a tight
    /// tactical update on a short gap, a full structural refresh on a long one — so
    /// the report is "gone about differently" by cadence. Always begins with the
    /// `Report cadence:` marker so the wiring is assertable. Fires even when the
    /// change view is absent (the first report), which the delta block cannot do.
    pub fn main_agent_guidance(&self) -> String {
        match self.bucket() {
            CadenceBucket::FirstReport => "Report cadence: this is the first Market Signal \
                report — there is no prior report to build on or audit. Establish the long-term \
                market thesis in full from the current baseline."
                .to_string(),
            CadenceBucket::Intraday => format!(
                "Report cadence: this report was generated less than a day after the previous \
                 one (~{:.1} days, an intraday cadence). Treat it as a tight tactical update — \
                 build directly on the prior thesis rather than re-deriving it, and concentrate \
                 on what actually moved in the hours since. Do not restate structural background \
                 that has not changed.",
                self.elapsed_days.unwrap_or(0.0)
            ),
            CadenceBucket::Daily => format!(
                "Report cadence: this report was generated about a day after the previous one \
                 (~{:.1} days, a daily cadence). Favor a focused update that carries the prior \
                 thesis forward and surfaces the latest incremental developments; reserve a full \
                 structural re-examination for a longer interval.",
                self.elapsed_days.unwrap_or(0.0)
            ),
            CadenceBucket::Weekly => format!(
                "Report cadence: this report was generated about {:.0} days after the previous \
                 one (a roughly weekly cadence — the standard interval). Synthesize what shaped \
                 markets over the interval and update the thesis at its usual structural depth.",
                self.elapsed_days.unwrap_or(0.0)
            ),
            CadenceBucket::Monthly => format!(
                "Report cadence: this report was generated about {:.0} days after the previous \
                 one (a roughly monthly cadence). A substantial interval has passed: re-examine \
                 the structural thesis more fully, test whether prior conclusions still hold, and \
                 weigh the broader set of developments since the last report.",
                self.elapsed_days.unwrap_or(0.0)
            ),
            CadenceBucket::Quarterly => format!(
                "Report cadence: this report was generated about {:.0} days after the previous \
                 one (a long gap). Treat it as a full structural refresh — do not assume \
                 continuity with the prior thesis; re-evaluate the market regime from the current \
                 baseline, since much may have shifted over this interval.",
                self.elapsed_days.unwrap_or(0.0)
            ),
        }
    }

    /// The shorter cadence cue prepended to each analyst's prompt. Coarser than the
    /// main agent's guidance — an analyst only needs to know whether to weight recent
    /// moves or the structural picture. Always begins with the `Report cadence:`
    /// marker so the wiring is assertable.
    pub fn analyst_cue(&self) -> String {
        match self.bucket() {
            CadenceBucket::FirstReport => "Report cadence: this is the first report — there is \
                no prior report to compare against; establish your perspective from the current \
                baseline."
                .to_string(),
            CadenceBucket::Intraday | CadenceBucket::Daily => format!(
                "Report cadence: a short interval (~{:.1} days) since the previous report — \
                 weight your review toward what changed recently rather than restating the full \
                 structural backdrop.",
                self.elapsed_days.unwrap_or(0.0)
            ),
            CadenceBucket::Weekly => format!(
                "Report cadence: roughly a week (~{:.0} days) since the previous report — a \
                 standard interval; weigh both the recent moves and the structural picture.",
                self.elapsed_days.unwrap_or(0.0)
            ),
            CadenceBucket::Monthly | CadenceBucket::Quarterly => format!(
                "Report cadence: a long interval (~{:.0} days) since the previous report — \
                 reassess the structural picture, not just the recent moves.",
                self.elapsed_days.unwrap_or(0.0)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_classifies_each_band() {
        assert_eq!(
            ReportCadence::from_elapsed(None).bucket(),
            CadenceBucket::FirstReport
        );
        assert_eq!(
            ReportCadence::from_elapsed(Some(0.25)).bucket(),
            CadenceBucket::Intraday
        );
        assert_eq!(
            ReportCadence::from_elapsed(Some(1.0)).bucket(),
            CadenceBucket::Daily,
            "the boundary value falls into the longer bucket (half-open bands)"
        );
        assert_eq!(
            ReportCadence::from_elapsed(Some(2.9)).bucket(),
            CadenceBucket::Daily
        );
        assert_eq!(
            ReportCadence::from_elapsed(Some(7.0)).bucket(),
            CadenceBucket::Weekly
        );
        assert_eq!(
            ReportCadence::from_elapsed(Some(14.0)).bucket(),
            CadenceBucket::Monthly,
            "the boundary value falls into the longer bucket"
        );
        assert_eq!(
            ReportCadence::from_elapsed(Some(30.0)).bucket(),
            CadenceBucket::Monthly
        );
        assert_eq!(
            ReportCadence::from_elapsed(Some(90.0)).bucket(),
            CadenceBucket::Quarterly
        );
    }

    #[test]
    fn negative_elapsed_from_clock_skew_floors_to_intraday() {
        // A previous run timestamped slightly ahead of this one must not panic or
        // mislabel — it lands in the shortest non-first bucket.
        assert_eq!(
            ReportCadence::from_elapsed(Some(-0.5)).bucket(),
            CadenceBucket::Intraday
        );
    }

    #[test]
    fn guidance_is_distinct_and_marked_per_bucket() {
        let samples = [None, Some(0.2), Some(1.0), Some(7.0), Some(30.0), Some(90.0)];
        let mut seen = std::collections::HashSet::new();
        for s in samples {
            let g = ReportCadence::from_elapsed(s).main_agent_guidance();
            assert!(
                g.starts_with("Report cadence:"),
                "guidance carries the marker: {g}"
            );
            assert!(
                seen.insert(g.clone()),
                "each bucket produces distinct guidance: {g}"
            );
        }
    }

    #[test]
    fn first_report_guidance_does_not_claim_a_prior() {
        let g = ReportCadence::from_elapsed(None).main_agent_guidance();
        assert!(g.contains("first"), "first-report guidance names the case: {g}");
        let cue = ReportCadence::from_elapsed(None).analyst_cue();
        assert!(cue.contains("first"), "first-report cue names the case: {cue}");
    }

    #[test]
    fn analyst_cue_collapses_to_three_live_registers_plus_first() {
        // Intraday and Daily share a cue; Monthly and Quarterly share a cue.
        let short_a = ReportCadence::from_elapsed(Some(0.2)).analyst_cue();
        let short_b = ReportCadence::from_elapsed(Some(2.0)).analyst_cue();
        let long_a = ReportCadence::from_elapsed(Some(30.0)).analyst_cue();
        let long_b = ReportCadence::from_elapsed(Some(90.0)).analyst_cue();
        // Same register, but the interpolated day count differs, so assert the shared
        // steer rather than string equality.
        assert!(short_a.contains("changed recently") && short_b.contains("changed recently"));
        assert!(long_a.contains("reassess") && long_b.contains("reassess"));
    }
}
