//! Step 9: the bounded research executor (`docs/weekly-report-workflow.md §Step 9`).
//!
//! The first consumer of the Step-8 `ResearchPlan`. Where routing decides *what*
//! to investigate, this stage executes it: it walks the plan's topics in priority
//! order, issues each query against a search backend, and returns the curated
//! evidence the main agent will eventually reason over (via the not-yet-built
//! Step-10 condensed packet).
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
//! evidence's consumer is the Step-10 condensed packet, which isn't built. The
//! *dynamic branching* itself ships as machinery only — the [`NoBranch`] default
//! does no branching; the follow-up generator (a model call or deterministic
//! rules keyed off the baseline deltas) is a deferred decision.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

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
/// against the budget, and the truncation reason (if any). What the Step-10
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

/// The wired default: no dynamic branching. The follow-up *generator* — a model
/// call or deterministic rules keyed off the baseline change view (`§Step 9`'s
/// "if oil spikes …" heuristics) — is a deferred decision; this no-op is the slot
/// the depth machinery is built around.
#[derive(Debug, Default)]
pub struct NoBranch;

impl BranchPolicy for NoBranch {
    fn follow_up(&self, _item: &ResearchItem, _finding: &Finding) -> Option<String> {
        None
    }
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
}
