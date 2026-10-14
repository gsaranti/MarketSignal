//! Report cadence: how long since the previous report, classified into the
//! bucket that sizes the data lookback windows and steers the agents. Reports
//! run on demand (`docs/scheduling.md §Generating a Report`), so the interval
//! is whatever the user chose. Computed in `pipeline::compute_cadence` from the
//! prior snapshot's `captured_at` alone, independent of the change view, so a
//! prior whose baseline payload won't decode still yields the true cadence.

/// Half-open band edges in days. App-layer tunables, not doc-pinned (the docs
/// fix no cadence vocabulary).
const INTRADAY_MAX_DAYS: f64 = 1.0;
const DAILY_MAX_DAYS: f64 = 3.0;
const WEEKLY_MAX_DAYS: f64 = 14.0;
const MONTHLY_MAX_DAYS: f64 = 55.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadenceBucket {
    /// No prior report.
    FirstReport,
    /// Less than a day since the previous report.
    Intraday,
    /// Roughly a day.
    Daily,
    /// A few days to ~two weeks — the standard interval.
    Weekly,
    /// Two weeks to ~two months.
    Monthly,
    /// Over ~two months.
    Quarterly,
}

/// `Default` is the first-report cadence, which is what an unset
/// `MainAgentInput.cadence` should mean.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReportCadence {
    elapsed_days: Option<f64>,
}

impl ReportCadence {
    /// `None` is the first report. A negative value (clock skew between runs)
    /// is floored to the first non-zero bucket by [`Self::bucket`].
    pub fn from_elapsed(elapsed_days: Option<f64>) -> Self {
        Self { elapsed_days }
    }

    /// Days, or `None` on the first report.
    pub fn elapsed_days(&self) -> Option<f64> {
        self.elapsed_days
    }

    /// The bands are half-open `[lo, hi)`, so each boundary value lands in the
    /// longer bucket.
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

    /// The cadence block appended to the main agent's prompt. Always begins
    /// with the `Report cadence:` marker so the wiring is assertable, and fires
    /// even when the change view is absent, which the delta block cannot do.
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

    /// The coarser cue prepended to each analyst's prompt. Always begins with
    /// the `Report cadence:` marker so the wiring is assertable.
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
        let short_a = ReportCadence::from_elapsed(Some(0.2)).analyst_cue();
        let short_b = ReportCadence::from_elapsed(Some(2.0)).analyst_cue();
        let long_a = ReportCadence::from_elapsed(Some(30.0)).analyst_cue();
        let long_b = ReportCadence::from_elapsed(Some(90.0)).analyst_cue();
        // The interpolated day count differs, so assert the shared steer, not equality.
        assert!(short_a.contains("changed recently") && short_b.contains("changed recently"));
        assert!(long_a.contains("reassess") && long_b.contains("reassess"));
    }
}
