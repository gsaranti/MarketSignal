//! Step 9: the bounded research executor (`docs/weekly-report-workflow.md §Step 9`).
//!
//! The first consumer of the Step-8 `ResearchPlan`. Where routing decides *what*
//! to investigate, this stage executes it: it walks the plan's topics in priority
//! order, issues each query against a search backend, and returns the curated
//! evidence the main agent will eventually reason over (via the not-yet-built
//! Step-11 condensed packet).
//!
//! This is the one stage that may loop or branch, so the three hard bounds live
//! here, in the application layer, not in any model: at most
//! [`MAX_RESEARCH_REQUESTS`] requests per job, at most [`MAX_RESEARCH_DURATION`]
//! of wall-clock, and a dynamic-branching depth of at most [`MAX_RESEARCH_DEPTH`]
//! (a query may spawn at most one follow-up). The budgets are checked at the
//! request boundary — the same cooperative-checkpoint pattern
//! `pipeline::bail_if_cancelled` uses for cancellation — so an in-flight request
//! is never interrupted, and a too-large plan degrades to a truncated-but-honest
//! [`ResearchEvidence`] (its `stopped_reason` records *why*) rather than aborting.
//!
//! Like the rest of the spine, the executor is synchronous: every backend HTTP
//! call is `reqwest::blocking`, and the whole stage runs inside the Tauri
//! command's `spawn_blocking`. The 30-minute bound is an elapsed-time check at
//! each boundary (the injectable [`Clock`] seam keeps it testable without
//! sleeping), not an async timeout — no `tokio` is needed here.
//!
//! Nothing is wired into the report pipeline yet (the Step-7/8 posture): the
//! evidence's consumer is the Step-11 condensed packet, which isn't built. The
//! *dynamic branching* ships as machinery only — [`NoBranch`] is the trait's
//! no-op default, and [`DeltaBranchPolicy`] is the real follow-up generator:
//! deterministic delta-rules keyed off the baseline change view (the §Step 9
//! "if oil spikes …" triggers). Selecting it over `NoBranch` at the
//! `execute_research` call site lands with the pipeline/Step-11 wiring.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::baseline_delta::{BaselineDeltas, Direction};
use crate::news::RawHeadline;
use crate::progress::RunContext;
use crate::research_router::{ResearchItem, ResearchPlan};

/// Hard ceiling on research requests per job (`docs/weekly-report-workflow.md
/// §Step 9`). Counted across every topic and every branching depth; the
/// `(N+1)`-th request is refused, so exactly this many searches can fire.
pub const MAX_RESEARCH_REQUESTS: usize = 50;

/// Hard wall-clock ceiling on the research phase (`§Step 9`). Checked at each
/// request boundary against the [`Clock`]; an in-flight request is allowed to
/// finish, so the real ceiling is this plus one backend timeout.
pub const MAX_RESEARCH_DURATION: Duration = Duration::from_secs(30 * 60);

/// Maximum dynamic-branching depth (`§Step 9`). A plan query runs at depth 1; a
/// finding below this cap may spawn one follow-up at depth 2; a depth-2 finding
/// spawns nothing further. The executor owns this cap — a [`BranchPolicy`] cannot
/// breach it.
pub const MAX_RESEARCH_DEPTH: u32 = 2;

/// Why the executor stopped before exhausting the plan. Absent from the evidence
/// (`stopped_reason == None`) means the whole plan ran within budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    /// The 50-request budget was reached.
    RequestBudget,
    /// The 30-minute wall-clock budget was reached.
    TimeBudget,
    /// The run was cancelled (cooperative flag observed at a boundary).
    Cancelled,
}

/// One executed query and the sources it returned, tagged with its branching
/// depth (1 for a plan query, 2 for a follow-up). A failed search records an
/// empty `sources` rather than dropping the finding, so the evidence shows the
/// query was attempted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub query: String,
    pub depth: u32,
    pub sources: Vec<RawHeadline>,
}

/// The curated evidence gathered for one plan topic: the originating topic and
/// rationale (carried through from the [`ResearchItem`]) plus every finding the
/// executor produced for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub topic: String,
    pub rationale: String,
    pub priority: f64,
    pub findings: Vec<Finding>,
}

/// The output of Step 9: the per-topic evidence, the total request count spent
/// against the budget, and the truncation reason (if any). What the Step-11
/// condensed packet will draw on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResearchEvidence {
    pub items: Vec<EvidenceItem>,
    pub requests_made: usize,
    pub stopped_reason: Option<StopReason>,
}

/// The executor's search backend: issue one research query and return its
/// sources. Sync and pure at the trait boundary, like `NewsSource` — the blocking
/// HTTP call inside the live (Tavily) adapter is offloaded via `spawn_blocking` at
/// the Tauri command seam. Distinct from `NewsSource::gather`, which is the fixed
/// Step-7 topic sweep; this drives arbitrary plan queries.
pub trait SearchBackend {
    fn search(&self, query: &str) -> anyhow::Result<Vec<RawHeadline>>;
}

/// Deterministic offline backend: echoes the query back as a single synthetic
/// source, so the executor and its tests run without live keys.
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

/// Decides whether a completed finding spawns a follow-up query (the depth-2
/// dynamic branching of `§Step 9`). Consulted only when the finding's depth is
/// below [`MAX_RESEARCH_DEPTH`], and the `Option` return type-enforces the spec's
/// "a research request may spawn at most one follow-up" — the executor owns the
/// depth cap, the one-per-request fan-out, *and* the request/time budgets, so no
/// policy can breach any of them. (The 50-request budget math the router relies on
/// — `research_router.rs`'s "5 topics × 4 × depth-2" — assumes exactly this 1:1
/// follow-up shape.)
pub trait BranchPolicy {
    fn follow_up(&self, item: &ResearchItem, finding: &Finding) -> Option<String>;
}

/// The trait's no-op default: no dynamic branching. The real follow-up generator is
/// [`DeltaBranchPolicy`] (deterministic delta-rules keyed off the change view); this
/// no-op remains the slot the depth machinery is built around and the stand-in
/// wherever no change view is available.
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

/// One delta-conditioned branching rule. When any of `series_ids` appears in this run's
/// change view having moved at least `threshold` (measured by `metric`) **in
/// `direction`**, the rule fires; it then contributes `follow_up_query` as the depth-2
/// second-order investigation for a research topic whose label or rationale contains any
/// of `keywords`. The direction gate keeps the rules faithful to the doc's *directional*
/// triggers — "if oil **spikes**", "if yields **rise sharply**"
/// (`docs/weekly-report-workflow.md §Step 9`) — so a sharp *decline* (a different thesis:
/// demand destruction, flight-to-quality) doesn't fire a rise-flavored follow-up. A
/// future crash rule is a one-row `Direction::Down` entry with its own query.
#[derive(Debug)]
struct TriggerRule {
    series_ids: &'static [&'static str],
    metric: Metric,
    threshold: f64,
    direction: Direction,
    keywords: &'static [&'static str],
    follow_up_query: &'static str,
}

/// The seeded delta-trigger table. Only delta-conditioned triggers over level-bearing
/// series the change view actually carries are expressible here, so the first cut ships
/// **oil** and **Treasury yields** — both FRED `internals` series, inside
/// `baseline_delta::DELTA_GROUPS`. The doc's other examples are deferred: geopolitical
/// escalation is news-conditioned (no delta signal), a semiconductor *price* level is
/// not in the delta groups (sector performance is excluded; only sector P/E is carried),
/// and "rally despite weak macro" is a compound index-vs-macro condition. Adding a clean
/// single-series trigger (dollar index, natural gas, a credit spread, VIX) is a one-row
/// edit here. Thresholds are raw-magnitude, calibrated to the ~weekly cadence (tunable).
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
            "yield", "yields", "treasury", "treasuries", "bond", "bonds", "duration",
            "10y", "2y", "30y",
        ],
        follow_up_query: "Implications of the recent rise in Treasury yields: Fed \
            rate-path repricing, inflation-expectation shifts, and bond-market and \
            funding stress.",
    },
];

/// The real follow-up generator (`docs/weekly-report-workflow.md §Step 9`): deterministic
/// delta-rules keyed off this run's baseline change view. Built via
/// [`DeltaBranchPolicy::from_deltas`], which resolves at construction which
/// [`TRIGGER_RULES`] fired (a tracked series moved past its threshold in the rule's
/// direction). During execution each fired rule emits its second-order follow-up query
/// **at most once**, attached to the first matching topic-finding the executor reaches —
/// and since the executor walks topics in descending priority, that is the
/// highest-priority matching topic. With no change view, or no rule fired, it degrades to
/// the [`NoBranch`] no-op.
///
/// One bound is inherited from the trait, not chosen here: `follow_up` returns a single
/// `Option<String>`, so a finding spawns at most one follow-up (the executor's
/// per-request budget math depends on this 1:1 shape). When several fired rules match the
/// *same* finding, their follow-up queries are **merged into one** combined query and all
/// are marked emitted — so every fired rule with a matching topic contributes exactly
/// once, without ever spawning a second search for one finding. (A fired rule that no
/// topic matches simply does not emit; there is nowhere on-thesis to attach it.)
///
/// Machinery only, like the rest of the research half: nothing selects it over
/// [`NoBranch`] at the `execute_research` call site yet (the executor is unwired from
/// `generate_report`); that selection lands with the pipeline/Step-11 wiring.
pub struct DeltaBranchPolicy {
    /// Indices into [`TRIGGER_RULES`] that fired for this run's change view.
    fired: Vec<usize>,
    /// Fired rules already emitted, so each contributes at most one follow-up across the
    /// whole run. Interior mutability because `follow_up` takes `&self`; the executor is
    /// single-threaded and synchronous, so a `RefCell` is sound here.
    emitted: RefCell<HashSet<usize>>,
}

impl DeltaBranchPolicy {
    /// Resolve the fired triggers from this run's change view. A rule fires when any of
    /// its `series_ids` appears in `deltas.changed` with a move at or past its `threshold`
    /// (by the rule's `metric`); a `Pct` rule needs a present `pct_change`.
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
        // Delta-conditioned, not finding-conditioned: the follow-up content comes from
        // which series moved (resolved at construction), not what this search returned —
        // so `_finding` is unused beyond driving the per-finding invocation.
        //
        // Every un-emitted fired rule this topic matches is consumed here and its query
        // merged into the single returned string: the trait yields one follow-up per
        // finding, so when two rules collide on one finding a merged query carries both
        // rather than dropping the lower-priority one.
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

/// Whether `rule` fired against `deltas`: any matching series id that moved in the rule's
/// `direction` and cleared the threshold by the rule's metric. The direction gate is what
/// keeps "oil spikes" / "yields rise sharply" from firing on a sharp decline.
fn rule_fired(rule: &TriggerRule, deltas: &BaselineDeltas) -> bool {
    deltas.changed.iter().any(|d| {
        rule.series_ids.contains(&d.id.as_str())
            && d.direction == rule.direction
            && match rule.metric {
                Metric::Abs => d.abs_change.abs() >= rule.threshold,
                Metric::Pct => d.pct_change.is_some_and(|p| p.abs() >= rule.threshold),
            }
    })
}

/// Whether any of `keywords` appears as a whole word in the item's topic or rationale.
/// Word-boundary (not substring) matching so a domain word like "turmoil" can't trip the
/// "oil" keyword; the text is split on non-alphanumeric characters so hyphenated labels
/// tokenize cleanly.
fn topic_matches(item: &ResearchItem, keywords: &[&str]) -> bool {
    let text = format!("{} {}", item.topic, item.rationale).to_lowercase();
    let words: HashSet<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    keywords.iter().any(|kw| words.contains(kw))
}

/// Elapsed-time source for the 30-minute budget. A seam so the budget is testable
/// without sleeping: the live [`WallClock`] wraps `Instant`; tests inject a clock
/// that advances deterministically.
pub trait Clock {
    /// Time elapsed since the research phase began.
    fn elapsed(&self) -> Duration;
}

/// The live clock: elapsed since construction. Built once, at the start of the
/// research phase.
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

/// Execute a bounded research plan against `backend`, enforcing the three Step-9
/// limits, and return the curated evidence.
///
/// Topics are taken in descending priority so the request budget is spent on the
/// most important ones first. Each topic's plan queries seed a work queue at depth
/// 1; `policy` may push one follow-up per finding (depth 2), bounded by
/// [`MAX_RESEARCH_DEPTH`]. Before every request the budget is checked — cancel
/// first, then the request count, then the clock — and the first breach sets
/// `stopped_reason` and returns what was gathered so far. Every actual search is
/// bracketed by a `request-started` / `request-finished` progress row, and a
/// failed search degrades to an empty finding rather than aborting the phase.
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

        // Per-topic work queue of (query, depth), seeded with the plan's queries at
        // depth 1. Follow-ups push depth+1 entries until the depth cap.
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
                    // Fail-soft: a single failed search records an empty finding and
                    // the phase continues.
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

/// The budget gate, evaluated at each request boundary. Cancel is checked first
/// (most urgent), then the request count, then the clock; the first breach wins.
/// `None` means another request may fire.
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

    /// Build a plan item with `n_queries` deterministic queries.
    fn item(topic: &str, n_queries: usize, priority: f64) -> ResearchItem {
        ResearchItem {
            topic: topic.into(),
            rationale: format!("why {topic}"),
            priority,
            queries: (0..n_queries).map(|i| format!("{topic} q{i}")).collect(),
        }
    }

    /// Real wall clock — its elapsed time in a test is microseconds, far under the
    /// 30-minute budget, so it never trips the time gate.
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

    /// Always spawns exactly one follow-up per finding — the test stand-in for the
    /// deferred real brancher, so the depth cap is exercisable.
    struct AlwaysBranch;

    impl BranchPolicy for AlwaysBranch {
        fn follow_up(&self, _item: &ResearchItem, finding: &Finding) -> Option<String> {
            Some(format!("follow-up to {}", finding.query))
        }
    }

    /// A reporter that flips the shared cancel flag on the first event it sees —
    /// i.e. during the first request's `request-started` — so the run cancels
    /// mid-phase, after one in-flight request has been allowed to finish.
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
        assert_eq!(evidence.requests_made, 3, "2 + 1 plan queries, no branching");
        assert_eq!(evidence.items.len(), 2);
        // Higher-priority topic first.
        assert_eq!(evidence.items[0].topic, "oil");
        assert_eq!(evidence.items[0].findings.len(), 2);
        assert_eq!(evidence.items[1].topic, "yields");
        assert_eq!(evidence.items[1].findings.len(), 1);
        // Every finding carries its query's stub source at depth 1.
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
        assert_eq!(findings, MAX_RESEARCH_REQUESTS, "one finding per request fired");
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
        let evidence =
            execute_research(&plan, &StubSearchBackend, &NoBranch, &clock, &RunContext::noop());

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

        let evidence =
            execute_research(&plan, &StubSearchBackend, &NoBranch, &fast_clock(), &ctx);

        assert_eq!(evidence.stopped_reason, Some(StopReason::Cancelled));
        // The first request fired (cancel flipped during its started-event); the
        // second boundary observed the cancel before issuing.
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

        // One depth-1 query spawns one depth-2 follow-up; the depth-2 finding spawns
        // nothing further.
        assert_eq!(evidence.requests_made, 2);
        let depths: Vec<u32> = evidence.items[0].findings.iter().map(|f| f.depth).collect();
        assert_eq!(depths, vec![1, 2]);
        assert!(
            evidence.items[0].findings.iter().all(|f| f.depth <= MAX_RESEARCH_DEPTH),
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
        assert_eq!(evidence.requests_made, 1, "no follow-ups without a brancher");
    }

    /// A `SeriesDelta` carrying just the fields a `TriggerRule` reads — its series id and
    /// move. The level fields are placeholders; the rules key off `id`, `abs_change`, and
    /// `pct_change` only.
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

    /// A change view over a ~weekly interval carrying the given level changes.
    fn deltas(changed: Vec<SeriesDelta>) -> BaselineDeltas {
        BaselineDeltas {
            elapsed_days: 7.0,
            changed,
            new: Vec::new(),
            missing: Vec::new(),
        }
    }

    /// A depth-1 finding stand-in for the direct `follow_up` unit tests — its content is
    /// irrelevant, since the delta-rules policy ignores it.
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
            .follow_up(&item("Energy / oil supply shock", 1, 0.9), &depth1_finding())
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
            .follow_up(&item("Energy / oil supply shock", 1, 0.9), &depth1_finding())
            .is_none());
    }

    #[test]
    fn delta_policy_fires_yields_above_its_bp_threshold() {
        // 30 bp on the 10y clears the 25 bp (0.25) absolute threshold.
        let policy =
            DeltaBranchPolicy::from_deltas(&deltas(vec![series_delta("DGS10", 0.30, Some(7.1))]));
        let q = policy
            .follow_up(&item("Treasury yields repricing", 1, 0.9), &depth1_finding())
            .expect("yield trigger fired and the topic matched");
        assert!(q.contains("Treasury yields"));
    }

    #[test]
    fn delta_policy_ignores_sharp_declines_for_rise_only_rules() {
        // An 8% oil *decline* and a 30 bp yield *decline* clear the magnitudes but move
        // the wrong way — the "spike" / "rise sharply" rules must stay silent rather than
        // emit a rise-flavored follow-up against a decline thesis.
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![
            series_delta("DCOILWTICO", -6.0, Some(-8.0)),
            series_delta("DGS10", -0.30, Some(-7.1)),
        ]));
        assert!(policy
            .follow_up(&item("Energy / oil supply shock", 1, 0.9), &depth1_finding())
            .is_none());
        assert!(policy
            .follow_up(&item("Treasury yields repricing", 1, 0.9), &depth1_finding())
            .is_none());
    }

    #[test]
    fn delta_policy_emits_distinct_fired_rules_on_their_own_topics() {
        // Both oil and yields rose past threshold this week; with a topic matching each,
        // both follow-ups emit — one per rule, on its own topic (the common case the
        // single-Option-per-finding bound does not constrain).
        let policy = DeltaBranchPolicy::from_deltas(&deltas(vec![
            series_delta("DCOILWTICO", 6.0, Some(8.0)),
            series_delta("DGS10", 0.30, Some(7.1)),
        ]));
        let oil = policy
            .follow_up(&item("oil supply shock", 1, 0.9), &depth1_finding())
            .expect("oil rule emits on the oil topic");
        assert!(oil.contains("oil"));
        let yields = policy
            .follow_up(&item("Treasury yields repricing", 1, 0.8), &depth1_finding())
            .expect("yield rule emits on the yields topic");
        assert!(yields.contains("Treasury yields"));
    }

    #[test]
    fn delta_policy_merges_rules_colliding_on_one_finding() {
        // Both oil and yields fired; a single-query cross-asset topic matches both keyword
        // sets. The one-follow-up-per-finding bound means one query must carry both — the
        // merged query covers each rule and neither is silently lost.
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
        assert!(q.contains("oil spike"), "merged query covers the oil rule: {q}");
        assert!(
            q.contains("Treasury yields"),
            "merged query covers the yield rule: {q}"
        );
        // Both rules are consumed — a later topic matching either gets nothing.
        assert!(policy
            .follow_up(&item("oil supply", 1, 0.8), &depth1_finding())
            .is_none());
        assert!(policy
            .follow_up(&item("bond duration", 1, 0.7), &depth1_finding())
            .is_none());
    }

    #[test]
    fn delta_policy_no_follow_up_when_topic_is_unrelated() {
        // Oil fired, but the topic is about labor — no keyword match, no follow-up.
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
        // Two oil-matching topics: the fired trigger attaches to the first, the second
        // gets nothing.
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
        // One depth-1 plan query spawns one depth-2 follow-up; the depth cap holds.
        assert_eq!(evidence.requests_made, 2);
        let depths: Vec<u32> = evidence.items[0].findings.iter().map(|f| f.depth).collect();
        assert_eq!(depths, vec![1, 2]);
        assert!(evidence.items[0]
            .findings
            .iter()
            .all(|f| f.depth <= MAX_RESEARCH_DEPTH));
    }
}
