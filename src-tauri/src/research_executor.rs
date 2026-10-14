//! Step 9: the bounded research executor (`docs/report-workflow.md §Step 9`). Walks the
//! Step-8 `ResearchPlan` against a [`SearchBackend`], branches through a [`BranchPolicy`],
//! and owns the request / wall-clock / depth bounds, checked at request boundaries so an
//! in-flight request is never interrupted and a too-large plan truncates honestly.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::baseline_delta::{BaselineDeltas, Direction};
use crate::news::RawHeadline;
use crate::progress::RunContext;
use crate::research_router::{ResearchItem, ResearchPlan};

/// Hard ceiling on research requests per job (`docs/report-workflow.md §Step 9`),
/// counted across every topic and branching depth; the `(N+1)`-th request is refused.
pub const MAX_RESEARCH_REQUESTS: usize = 50;

/// Hard wall-clock ceiling on the research phase (`§Step 9`), checked at each request
/// boundary; an in-flight request finishes, so the real ceiling is this plus one backend
/// timeout.
pub const MAX_RESEARCH_DURATION: Duration = Duration::from_secs(30 * 60);

/// Maximum dynamic-branching depth (`§Step 9`): a plan query runs at depth 1 and may
/// spawn one follow-up at depth 2. The executor owns this cap; a [`BranchPolicy`] cannot
/// breach it.
pub const MAX_RESEARCH_DEPTH: u32 = 2;

/// Why the executor stopped before exhausting the plan; `None` on the evidence means the
/// whole plan ran within budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    RequestBudget,
    TimeBudget,
    /// The cooperative cancellation flag was observed at a request boundary.
    Cancelled,
}

/// One executed query and the sources it returned; `depth` is 1 for a plan query, 2 for
/// a follow-up. A failed search records empty `sources` rather than dropping the
/// finding, so the evidence shows the query was attempted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub query: String,
    pub depth: u32,
    pub sources: Vec<RawHeadline>,
}

/// One plan topic's evidence, the topic and rationale carried from its [`ResearchItem`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub topic: String,
    pub rationale: String,
    pub priority: f64,
    pub findings: Vec<Finding>,
}

/// The output of Step 9; `requests_made` is the count spent against the request budget.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResearchEvidence {
    pub items: Vec<EvidenceItem>,
    pub requests_made: usize,
    pub stopped_reason: Option<StopReason>,
}

/// The executor's search backend. Sync at the trait boundary, like `NewsSource`: the
/// live adapter's blocking HTTP call is offloaded via `spawn_blocking` at the Tauri
/// command seam. Distinct from `NewsSource::gather`, the fixed Step-7 topic sweep; this
/// drives arbitrary plan queries.
pub trait SearchBackend {
    fn search(&self, query: &str) -> anyhow::Result<Vec<RawHeadline>>;
}

/// Deterministic offline backend: echoes the query back as one synthetic source.
#[derive(Debug, Default)]
pub struct StubSearchBackend;

impl SearchBackend for StubSearchBackend {
    fn search(&self, query: &str) -> anyhow::Result<Vec<RawHeadline>> {
        Ok(vec![RawHeadline {
            title: format!("Result for: {query}"),
            url: format!("https://example.com/research?q={}", query.replace(' ', "+")),
            source: "example.com".into(),
            published: None,
            snippet: Some(format!("Synthetic research evidence for {query}")),
        }])
    }
}

/// Decides whether a completed finding spawns a follow-up query (`§Step 9`). Consulted
/// only when the finding's depth is below [`MAX_RESEARCH_DEPTH`]; the `Option` return
/// type-enforces "at most one follow-up per request", the 1:1 shape the router's
/// request-budget math ("5 topics × 4 × depth-2") assumes. The executor owns the depth
/// cap and the budgets, so no policy can breach them.
pub trait BranchPolicy {
    fn follow_up(&self, item: &ResearchItem, finding: &Finding) -> Option<String>;
}

/// No dynamic branching: the stand-in wherever no change view is available.
#[derive(Debug, Default)]
pub struct NoBranch;

impl BranchPolicy for NoBranch {
    fn follow_up(&self, _item: &ResearchItem, _finding: &Finding) -> Option<String> {
        None
    }
}

/// Which series metric a [`TriggerRule`] thresholds against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    /// Absolute level change in the series' natural unit — for a Treasury yield
    /// (quoted in percent) `0.25` is a 25 bp move. Always present on a `SeriesDelta`.
    Abs,
    /// Percent change off the prior level, in the `baseline_delta` convention where a
    /// +7% move is `7.0` (not `0.07`). Skipped when `pct_change` is absent (the prior
    /// level was zero or non-finite).
    Pct,
}

/// One delta-conditioned branching rule: fires when any of `series_ids` moved at least
/// `threshold` (by `metric`) in `direction` this run, and then contributes
/// `follow_up_query` as the depth-2 follow-up for a topic whose label or rationale
/// contains any of `keywords`. The direction gate keeps the rules faithful to the doc's
/// directional triggers ("if oil spikes", `docs/report-workflow.md §Step 9`), so a sharp
/// decline — a different thesis — doesn't fire a rise-flavored follow-up.
#[derive(Debug)]
struct TriggerRule {
    series_ids: &'static [&'static str],
    metric: Metric,
    threshold: f64,
    direction: Direction,
    keywords: &'static [&'static str],
    follow_up_query: &'static str,
}

/// The cadence anchor for the trigger thresholds, in days: each rule's `threshold` is
/// the move that fires over a weekly interval, scaled by [`cadence_scale`] to the run's
/// actual elapsed interval. Falls inside the cadence module's `Weekly` band.
const THRESHOLD_ANCHOR_DAYS: f64 = 7.0;

/// Clamp floor on the cadence scale factor: keeps a near-zero interval (an intraday
/// regeneration, or clock skew yielding a non-positive elapsed) from collapsing the
/// threshold onto market noise.
const THRESHOLD_SCALE_MIN: f64 = 0.5;

/// Clamp ceiling on the cadence scale factor: keeps a long gap from raising the threshold
/// so far that a rule can never fire.
const THRESHOLD_SCALE_MAX: f64 = 2.5;

/// Time-normalize a weekly-anchored threshold to a run's cadence:
/// `sqrt(elapsed_days / THRESHOLD_ANCHOR_DAYS)`, clamped. The square root is the
/// random-walk law (a series' expected move grows with the square root of elapsed time),
/// so a move must be big for its interval to fire; at the weekly anchor the factor is
/// `1.0`. A non-positive or non-finite interval floors to `THRESHOLD_SCALE_MIN` before
/// the clamp, because `f64::clamp` would propagate a `NaN` straight through.
fn cadence_scale(elapsed_days: f64) -> f64 {
    let ratio = elapsed_days / THRESHOLD_ANCHOR_DAYS;
    if !ratio.is_finite() || ratio <= 0.0 {
        return THRESHOLD_SCALE_MIN;
    }
    ratio.sqrt().clamp(THRESHOLD_SCALE_MIN, THRESHOLD_SCALE_MAX)
}

/// The delta-trigger table. Only delta-conditioned triggers over series the change view
/// carries (`baseline_delta::DELTA_GROUPS`) are expressible here, which is why the doc's
/// news-conditioned and compound examples (geopolitical escalation, "rally despite weak
/// macro") are absent; a single-series trigger is a one-row edit. Each `threshold` is the
/// weekly-anchored magnitude, scaled to the run's cadence by [`rule_fired`].
const TRIGGER_RULES: &[TriggerRule] = &[
    TriggerRule {
        series_ids: &["DCOILWTICO"],
        metric: Metric::Pct,
        threshold: 7.0,
        direction: Direction::Up,
        keywords: &["oil", "crude", "wti", "brent", "opec", "petroleum"],
        follow_up_query: "Second-order effects of the recent oil spike: inflation \
            pass-through, shipping and freight costs, supply-chain disruption, and \
            geopolitical energy-supply risk.",
    },
    TriggerRule {
        series_ids: &["DGS10"],
        metric: Metric::Abs,
        threshold: 0.25,
        direction: Direction::Up,
        keywords: &[
            "yield",
            "yields",
            "treasury",
            "treasuries",
            "bond",
            "bonds",
            "duration",
            "10y",
            "2y",
            "30y",
        ],
        follow_up_query: "Implications of the recent rise in Treasury yields: Fed \
            rate-path repricing, inflation-expectation shifts, and bond-market and \
            funding stress.",
    },
];

/// The follow-up generator (`docs/report-workflow.md §Step 9`): deterministic
/// delta-rules keyed off this run's change view, the fired [`TRIGGER_RULES`] resolved at
/// construction. Each fired rule emits its follow-up at most once, attached to the first
/// matching topic-finding the executor reaches — the highest-priority one, since topics
/// walk in descending priority. Several fired rules matching the same finding merge into
/// one combined query, because the trait yields one follow-up per finding; a fired rule
/// no topic matches never emits.
pub struct DeltaBranchPolicy {
    /// Indices into [`TRIGGER_RULES`].
    fired: Vec<usize>,
    /// Fired rules already emitted. Interior mutability because `follow_up` takes
    /// `&self`; the executor is single-threaded and synchronous, so a `RefCell` is sound.
    emitted: RefCell<HashSet<usize>>,
}

impl DeltaBranchPolicy {
    pub fn from_deltas(deltas: &BaselineDeltas) -> Self {
        let fired = TRIGGER_RULES
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule_fired(rule, deltas))
            .map(|(i, _)| i)
            .collect();
        Self {
            fired,
            emitted: RefCell::new(HashSet::new()),
        }
    }
}

impl BranchPolicy for DeltaBranchPolicy {
    fn follow_up(&self, item: &ResearchItem, _finding: &Finding) -> Option<String> {
        // Delta-conditioned, not finding-conditioned: the follow-up comes from which
        // series moved, so `_finding` only drives the per-finding invocation. Every
        // un-emitted fired rule this topic matches is consumed here and merged into the
        // one returned query.
        let mut emitted = self.emitted.borrow_mut();
        let mut queries: Vec<&'static str> = Vec::new();
        for &idx in &self.fired {
            if emitted.contains(&idx) {
                continue;
            }
            let rule = &TRIGGER_RULES[idx];
            if topic_matches(item, rule.keywords) {
                emitted.insert(idx);
                queries.push(rule.follow_up_query);
            }
        }
        if queries.is_empty() {
            None
        } else {
            Some(queries.join(" "))
        }
    }
}

/// [`DeltaBranchPolicy`] when a change view exists, otherwise [`NoBranch`] (a first
/// report or an unreadable prior snapshot yields `None`).
pub fn select_branch_policy(deltas: Option<&BaselineDeltas>) -> Box<dyn BranchPolicy> {
    match deltas {
        Some(deltas) => Box::new(DeltaBranchPolicy::from_deltas(deltas)),
        None => Box::new(NoBranch),
    }
}

/// Any matching series moved in the rule's `direction` past the cadence-scaled threshold
/// by the rule's metric.
fn rule_fired(rule: &TriggerRule, deltas: &BaselineDeltas) -> bool {
    let threshold = rule.threshold * cadence_scale(deltas.elapsed_days);
    deltas.changed.iter().any(|d| {
        rule.series_ids.contains(&d.id.as_str())
            && d.direction == rule.direction
            && match rule.metric {
                Metric::Abs => d.abs_change.abs() >= threshold,
                Metric::Pct => d.pct_change.is_some_and(|p| p.abs() >= threshold),
            }
    })
}

/// Whole-word (not substring) matching, so "turmoil" can't trip the "oil" keyword; the
/// text splits on non-alphanumeric characters so hyphenated labels tokenize cleanly.
fn topic_matches(item: &ResearchItem, keywords: &[&str]) -> bool {
    let text = format!("{} {}", item.topic, item.rationale).to_lowercase();
    let words: HashSet<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    keywords.iter().any(|kw| words.contains(kw))
}

/// Elapsed-time source for the wall-clock budget: a seam so the budget is testable
/// without sleeping.
pub trait Clock {
    /// Since the research phase began.
    fn elapsed(&self) -> Duration;
}

/// Elapsed since construction; built once at the start of the research phase.
pub struct WallClock {
    start: Instant,
}

impl WallClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for WallClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for WallClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Execute a bounded research plan against `backend` under the three Step-9 limits.
/// Topics are taken in descending priority so the request budget is spent on the most
/// important ones first. Before every request the budget is checked — cancel first,
/// then the request count, then the clock — and the first breach sets `stopped_reason`
/// and returns what was gathered so far. A failed search degrades to an empty finding
/// rather than aborting the phase.
pub fn execute_research(
    plan: &ResearchPlan,
    backend: &dyn SearchBackend,
    policy: &dyn BranchPolicy,
    clock: &dyn Clock,
    ctx: &RunContext,
) -> ResearchEvidence {
    let mut evidence = ResearchEvidence::default();

    let mut items: Vec<&ResearchItem> = plan.items.iter().collect();
    items.sort_by(|a, b| b.priority.total_cmp(&a.priority));

    'outer: for item in items {
        let mut evidence_item = EvidenceItem {
            topic: item.topic.clone(),
            rationale: item.rationale.clone(),
            priority: item.priority,
            findings: Vec::new(),
        };

        let mut queue: VecDeque<(String, u32)> =
            item.queries.iter().map(|q| (q.clone(), 1)).collect();

        while let Some((query, depth)) = queue.pop_front() {
            if let Some(reason) = check_budget(&evidence, clock, ctx) {
                evidence.stopped_reason = Some(reason);
                evidence.items.push(evidence_item);
                break 'outer;
            }

            // One progress row per actual HTTP call (the run-tracking invariant).
            ctx.request_started("Tavily", "research", item.topic.as_str(), query.as_str());
            evidence.requests_made += 1;
            let finding = match backend.search(&query) {
                Ok(sources) => {
                    ctx.request_finished(
                        "Tavily",
                        "research",
                        item.topic.as_str(),
                        query.as_str(),
                        "ok",
                        None,
                    );
                    Finding {
                        query: query.clone(),
                        depth,
                        sources,
                    }
                }
                Err(e) => {
                    ctx.request_finished(
                        "Tavily",
                        "research",
                        item.topic.as_str(),
                        query.as_str(),
                        "failed",
                        Some(e.to_string()),
                    );
                    // Fail-soft: an empty finding, and the phase continues.
                    Finding {
                        query: query.clone(),
                        depth,
                        sources: Vec::new(),
                    }
                }
            };

            if depth < MAX_RESEARCH_DEPTH {
                if let Some(follow) = policy.follow_up(item, &finding) {
                    queue.push_back((follow, depth + 1));
                }
            }

            evidence_item.findings.push(finding);
        }

        evidence.items.push(evidence_item);
    }

    evidence
}

/// Cancel first (most urgent), then the request count, then the clock; `None` means
/// another request may fire.
fn check_budget(
    evidence: &ResearchEvidence,
    clock: &dyn Clock,
    ctx: &RunContext,
) -> Option<StopReason> {
    if ctx.is_cancelled() {
        return Some(StopReason::Cancelled);
    }
    if evidence.requests_made >= MAX_RESEARCH_REQUESTS {
        return Some(StopReason::RequestBudget);
    }
    if clock.elapsed() >= MAX_RESEARCH_DURATION {
        return Some(StopReason::TimeBudget);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use crate::progress::{ProgressMessage, ProgressReporter, RunContext};

    use crate::baseline_delta::{BaselineDeltas, Direction, SeriesDelta};
    use crate::data_sources::GroupKind;

    fn item(topic: &str, n_queries: usize, priority: f64) -> ResearchItem {
        ResearchItem {
            topic: topic.into(),
            rationale: format!("why {topic}"),
            priority,
            queries: (0..n_queries).map(|i| format!("{topic} q{i}")).collect(),
        }
    }

    /// The real clock: microseconds elapsed in a test, so it never trips the time gate.
    fn fast_clock() -> WallClock {
        WallClock::new()
    }

    /// A clock that jumps `step` seconds every time it is read, so a fixed number
    /// of request boundaries deterministically crosses the time budget.
    struct SteppingClock {
        secs: Cell<u64>,
        step: u64,
    }

    impl Clock for SteppingClock {
        fn elapsed(&self) -> Duration {
            let now = self.secs.get();
            self.secs.set(now + self.step);
            Duration::from_secs(now)
        }
    }

    /// One follow-up per finding, so the depth cap is exercisable.
    struct AlwaysBranch;

    impl BranchPolicy for AlwaysBranch {
        fn follow_up(&self, _item: &ResearchItem, finding: &Finding) -> Option<String> {
            Some(format!("follow-up to {}", finding.query))
        }
    }

    /// Flips the cancel flag on the first event it sees (the first request's
    /// `request-started`), so the run cancels after one in-flight request finishes.
    struct CancelOnFirstEvent {
        cancel: Arc<AtomicBool>,
        fired: AtomicBool,
    }

    impl ProgressReporter for CancelOnFirstEvent {
        fn report(&self, _message: &ProgressMessage) {
            if !self.fired.swap(true, Ordering::Relaxed) {
                self.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    #[test]
    fn happy_path_returns_evidence_per_topic_within_budget() {
        let plan = ResearchPlan {
            items: vec![item("oil", 2, 0.9), item("yields", 1, 0.5)],
        };
        let evidence = execute_research(
            &plan,
            &StubSearchBackend,
            &NoBranch,
            &fast_clock(),
            &RunContext::noop(),
        );

        assert_eq!(evidence.stopped_reason, None, "full plan ran within budget");
        assert_eq!(
            evidence.requests_made, 3,
            "2 + 1 plan queries, no branching"
        );
        assert_eq!(evidence.items.len(), 2);
        assert_eq!(evidence.items[0].topic, "oil");
        assert_eq!(evidence.items[0].findings.len(), 2);
        assert_eq!(evidence.items[1].topic, "yields");
        assert_eq!(evidence.items[1].findings.len(), 1);
        assert!(evidence
            .items
            .iter()
            .flat_map(|i| &i.findings)
            .all(|f| f.depth == 1 && f.sources.len() == 1));
    }

    #[test]
    fn request_budget_caps_the_run_at_fifty() {
        // 5 topics x 11 queries = 55 plan queries, more than the 50 budget.
        let plan = ResearchPlan {
            items: (0..5)
                .map(|i| item(&format!("topic{i}"), 11, 1.0))
                .collect(),
        };
        let evidence = execute_research(
            &plan,
            &StubSearchBackend,
            &NoBranch,
            &fast_clock(),
            &RunContext::noop(),
        );

        assert_eq!(evidence.stopped_reason, Some(StopReason::RequestBudget));
        assert_eq!(evidence.requests_made, MAX_RESEARCH_REQUESTS);
        let findings: usize = evidence.items.iter().map(|i| i.findings.len()).sum();
        assert_eq!(
            findings, MAX_RESEARCH_REQUESTS,
            "one finding per request fired"
        );
    }

    #[test]
    fn time_budget_stops_the_run() {
        // Each boundary read jumps 11 minutes; the 30-minute budget is crossed on
        // the fourth read, after three requests have fired.
        let clock = SteppingClock {
            secs: Cell::new(0),
            step: 11 * 60,
        };
        let plan = ResearchPlan {
            items: vec![item("oil", 5, 1.0)],
        };
        let evidence = execute_research(
            &plan,
            &StubSearchBackend,
            &NoBranch,
            &clock,
            &RunContext::noop(),
        );

        assert_eq!(evidence.stopped_reason, Some(StopReason::TimeBudget));
        assert_eq!(evidence.requests_made, 3);
    }

    #[test]
    fn cancellation_stops_the_run_after_the_in_flight_request() {
        let cancel = Arc::new(AtomicBool::new(false));
        let reporter = Arc::new(CancelOnFirstEvent {
            cancel: cancel.clone(),
            fired: AtomicBool::new(false),
        });
        let ctx = RunContext::new("cancel-test", reporter, cancel);
        let plan = ResearchPlan {
            items: vec![item("oil", 3, 1.0)],
        };

        let evidence = execute_research(&plan, &StubSearchBackend, &NoBranch, &fast_clock(), &ctx);

        assert_eq!(evidence.stopped_reason, Some(StopReason::Cancelled));
        // Cancel flipped during the first request's started-event; the second boundary saw it.
        assert_eq!(evidence.requests_made, 1);
    }

    #[test]
    fn branching_stops_at_depth_two() {
        let plan = ResearchPlan {
            items: vec![item("oil", 1, 1.0)],
        };
        let evidence = execute_research(
            &plan,
            &StubSearchBackend,
            &AlwaysBranch,
            &fast_clock(),
            &RunContext::noop(),
        );

        assert_eq!(evidence.requests_made, 2);
        let depths: Vec<u32> = evidence.items[0].findings.iter().map(|f| f.depth).collect();
        assert_eq!(depths, vec![1, 2]);
        assert!(
            evidence.items[0]
                .findings
                .iter()
                .all(|f| f.depth <= MAX_RESEARCH_DEPTH),
            "no finding exceeds the depth cap"
        );
    }

    #[test]
    fn no_branch_policy_issues_only_the_plan_queries() {
        let plan = ResearchPlan {
            items: vec![item("oil", 1, 1.0)],
        };
        let evidence = execute_research(
            &plan,
            &StubSearchBackend,
            &NoBranch,
            &fast_clock(),
            &RunContext::noop(),
        );
        assert_eq!(
            evidence.requests_made, 1,
            "no follow-ups without a brancher"
        );
    }

    /// The level fields are placeholders; the rules never read them.
    fn series_delta(id: &str, abs_change: f64, pct_change: Option<f64>) -> SeriesDelta {
        SeriesDelta {
            group: GroupKind::Internals,
            id: id.into(),
            name: id.into(),
            current: 0.0,
            prior: 0.0,
            abs_change,
            pct_change,
            direction: if abs_change >= 0.0 {
                Direction::Up
            } else {
                Direction::Down
            },
        }
    }

    /// A change view over the weekly anchor, so `cadence_scale` is `1.0` and the tests see
    /// the unscaled thresholds.
    fn deltas(changed: Vec<SeriesDelta>) -> BaselineDeltas {
        deltas_over(changed, 7.0)
    }

    fn deltas_over(changed: Vec<SeriesDelta>, elapsed_days: f64) -> BaselineDeltas {
        BaselineDeltas {
            elapsed_days,
            changed,
            new: Vec::new(),
            missing: Vec::new(),
        }
    }

    /// Content irrelevant: the delta-rules policy ignores the finding.
    fn depth1_finding() -> Finding {
        Finding {
            query: "q".into(),
            depth: 1,
            sources: Vec::new(),
        }
    }

    #[test]
    fn delta_policy_fires_oil_above_its_pct_threshold() {
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta(
            "DCOILWTICO",
            6.0,
            Some(8.0),
        )]));
        let q = policy
            .follow_up(
                &item("Energy / oil supply shock", 1, 0.9),
                &depth1_finding(),
            )
            .expect("oil trigger fired and the topic matched");
        assert!(q.starts_with("Second-order effects of the recent oil spike"));
    }

    #[test]
    fn delta_policy_silent_below_oil_threshold() {
        // A 3% oil move is below the 7% spike threshold.
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta(
            "DCOILWTICO",
            2.0,
            Some(3.0),
        )]));
        assert!(policy
            .follow_up(
                &item("Energy / oil supply shock", 1, 0.9),
                &depth1_finding()
            )
            .is_none());
    }

    #[test]
    fn delta_policy_fires_yields_above_its_bp_threshold() {
        // 30 bp on the 10y clears the 25 bp (0.25) absolute threshold.
        let policy =
            DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta("DGS10", 0.30, Some(7.1))]));
        let q = policy
            .follow_up(
                &item("Treasury yields repricing", 1, 0.9),
                &depth1_finding(),
            )
            .expect("yield trigger fired and the topic matched");
        assert!(q.contains("Treasury yields"));
    }

    #[test]
    fn delta_policy_ignores_sharp_declines_for_rise_only_rules() {
        // The magnitudes clear the thresholds but move the wrong way.
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![
            series_delta("DCOILWTICO", -6.0, Some(-8.0)),
            series_delta("DGS10", -0.30, Some(-7.1)),
        ]));
        assert!(policy
            .follow_up(
                &item("Energy / oil supply shock", 1, 0.9),
                &depth1_finding()
            )
            .is_none());
        assert!(policy
            .follow_up(
                &item("Treasury yields repricing", 1, 0.9),
                &depth1_finding()
            )
            .is_none());
    }

    #[test]
    fn delta_policy_emits_distinct_fired_rules_on_their_own_topics() {
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![
            series_delta("DCOILWTICO", 6.0, Some(8.0)),
            series_delta("DGS10", 0.30, Some(7.1)),
        ]));
        let oil = policy
            .follow_up(&item("oil supply shock", 1, 0.9), &depth1_finding())
            .expect("oil rule emits on the oil topic");
        assert!(oil.contains("oil"));
        let yields = policy
            .follow_up(
                &item("Treasury yields repricing", 1, 0.8),
                &depth1_finding(),
            )
            .expect("yield rule emits on the yields topic");
        assert!(yields.contains("Treasury yields"));
    }

    #[test]
    fn delta_policy_merges_rules_colliding_on_one_finding() {
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![
            series_delta("DCOILWTICO", 6.0, Some(8.0)),
            series_delta("DGS10", 0.30, Some(7.1)),
        ]));
        let q = policy
            .follow_up(
                &item("Cross-asset stress: oil and Treasury yields", 1, 0.9),
                &depth1_finding(),
            )
            .expect("a topic matching both fired rules emits a merged follow-up");
        assert!(
            q.contains("oil spike"),
            "merged query covers the oil rule: {q}"
        );
        assert!(
            q.contains("Treasury yields"),
            "merged query covers the yield rule: {q}"
        );
        // Both rules are consumed.
        assert!(policy
            .follow_up(&item("oil supply", 1, 0.8), &depth1_finding())
            .is_none());
        assert!(policy
            .follow_up(&item("bond duration", 1, 0.7), &depth1_finding())
            .is_none());
    }

    #[test]
    fn delta_policy_no_follow_up_when_topic_is_unrelated() {
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta(
            "DCOILWTICO",
            6.0,
            Some(8.0),
        )]));
        assert!(policy
            .follow_up(&item("Labor market softening", 1, 0.9), &depth1_finding())
            .is_none());
    }

    #[test]
    fn delta_policy_emits_each_fired_trigger_once() {
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta(
            "DCOILWTICO",
            6.0,
            Some(8.0),
        )]));
        assert!(policy
            .follow_up(&item("oil supply", 1, 0.9), &depth1_finding())
            .is_some());
        assert!(
            policy
                .follow_up(&item("crude oil prices", 1, 0.8), &depth1_finding())
                .is_none(),
            "each fired trigger contributes at most one follow-up"
        );
    }

    #[test]
    fn delta_policy_with_no_fired_trigger_is_a_noop() {
        let policy = DeltaBranchPolicy::from_deltas(&deltas(Vec::new()));
        assert!(policy
            .follow_up(&item("oil supply", 1, 0.9), &depth1_finding())
            .is_none());
    }

    #[test]
    fn delta_policy_matches_on_word_boundaries_not_substrings() {
        // Oil fired, but "turmoil" must not match the "oil" keyword.
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta(
            "DCOILWTICO",
            6.0,
            Some(8.0),
        )]));
        assert!(policy
            .follow_up(
                &item("Geopolitical turmoil in Europe", 1, 0.9),
                &depth1_finding()
            )
            .is_none());
    }

    #[test]
    fn delta_policy_branches_through_the_executor() {
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta(
            "DCOILWTICO",
            6.0,
            Some(8.0),
        )]));
        let plan = ResearchPlan {
            items: vec![item("oil supply shock", 1, 1.0)],
        };
        let evidence = execute_research(
            &plan,
            &StubSearchBackend,
            &policy,
            &fast_clock(),
            &RunContext::noop(),
        );
        assert_eq!(evidence.requests_made, 2);
        let depths: Vec<u32> = evidence.items[0].findings.iter().map(|f| f.depth).collect();
        assert_eq!(depths, vec![1, 2]);
        assert!(evidence.items[0]
            .findings
            .iter()
            .all(|f| f.depth <= MAX_RESEARCH_DEPTH));
    }

    #[test]
    fn select_branch_policy_picks_delta_policy_when_a_change_view_is_present() {
        let view = deltas(vec![series_delta("DGS10", 0.30, Some(7.1))]);
        let policy = select_branch_policy(Some(&view));
        let q = policy
            .follow_up(
                &item("Treasury yields repricing", 1, 0.9),
                &depth1_finding(),
            )
            .expect("delta policy fired the yield rule");
        assert!(q.contains("Treasury yields"));
    }

    #[test]
    fn select_branch_policy_falls_back_to_nobranch_without_a_change_view() {
        let policy = select_branch_policy(None);
        assert!(policy
            .follow_up(
                &item("Treasury yields repricing", 1, 0.9),
                &depth1_finding()
            )
            .is_none());
    }

    // --- Cadence scaling of the trigger thresholds (sqrt-of-time, weekly-anchored) ---

    #[test]
    fn cadence_scale_is_unity_at_the_weekly_anchor() {
        assert!((cadence_scale(THRESHOLD_ANCHOR_DAYS) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cadence_scale_grows_with_the_interval() {
        // A 4x interval is a 2x factor under the square-root law.
        assert!(cadence_scale(1.75) < cadence_scale(7.0));
        assert!(cadence_scale(7.0) < cadence_scale(28.0));
        assert!((cadence_scale(28.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn cadence_scale_floors_on_degenerate_intervals() {
        for bad in [0.01, 0.0, -5.0, f64::NAN] {
            assert_eq!(
                cadence_scale(bad),
                THRESHOLD_SCALE_MIN,
                "degenerate interval {bad} floors to THRESHOLD_SCALE_MIN"
            );
        }
    }

    #[test]
    fn cadence_scale_caps_on_a_long_gap() {
        // sqrt(365/7) ~= 7.2, clamped to the ceiling.
        assert_eq!(cadence_scale(365.0), THRESHOLD_SCALE_MAX);
    }

    #[test]
    fn delta_policy_suppresses_a_weekly_move_over_a_long_gap() {
        // Over a 30-day gap the 7% threshold scales to ~14.5% (sqrt(30/7) ~= 2.07).
        let weekly = DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta(
            "DCOILWTICO",
            6.0,
            Some(8.0),
        )]));
        assert!(
            weekly
                .follow_up(&item("oil supply shock", 1, 0.9), &depth1_finding())
                .is_some(),
            "8% oil fires over a weekly interval"
        );
        let monthly = DeltaBranchPolicy::from_deltas(&deltas_over(
            vec![series_delta("DCOILWTICO", 6.0, Some(8.0))],
            30.0,
        ));
        assert!(
            monthly
                .follow_up(&item("oil supply shock", 1, 0.9), &depth1_finding())
                .is_none(),
            "the same 8% move does not fire over a 30-day gap"
        );
    }

    #[test]
    fn delta_policy_fires_a_sub_weekly_move_over_a_short_gap() {
        // Over a 1-day gap the 7% threshold scales to 3.5% (sqrt(1/7) ~= 0.38, clamped
        // to the 0.5 floor).
        let weekly = DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta(
            "DCOILWTICO",
            3.0,
            Some(4.0),
        )]));
        assert!(
            weekly
                .follow_up(&item("oil supply shock", 1, 0.9), &depth1_finding())
                .is_none(),
            "4% oil does not fire over a weekly interval"
        );
        let daily = DeltaBranchPolicy::from_deltas(&deltas_over(
            vec![series_delta("DCOILWTICO", 3.0, Some(4.0))],
            1.0,
        ));
        assert!(
            daily
                .follow_up(&item("oil supply shock", 1, 0.9), &depth1_finding())
                .is_some(),
            "the same 4% move fires over a 1-day gap"
        );
    }

    #[test]
    fn delta_policy_scales_the_absolute_yield_threshold_too() {
        // 25 bp scales to 12.5 bp over a 1-day gap (the 0.5 floor) and ~51.7 bp over 30
        // days.
        let daily = DeltaBranchPolicy::from_deltas(&deltas_over(
            vec![series_delta("DGS10", 0.20, Some(4.5))],
            1.0,
        ));
        assert!(
            daily
                .follow_up(&item("Treasury yields repricing", 1, 0.9), &depth1_finding())
                .is_some(),
            "20 bp fires over a 1-day gap"
        );
        let monthly = DeltaBranchPolicy::from_deltas(&deltas_over(
            vec![series_delta("DGS10", 0.20, Some(4.5))],
            30.0,
        ));
        assert!(
            monthly
                .follow_up(&item("Treasury yields repricing", 1, 0.9), &depth1_finding())
                .is_none(),
            "the same 20 bp move does not fire over a 30-day gap"
        );
    }
}
