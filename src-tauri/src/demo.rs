//! Demo-run mode — dev-only, feature `demo-run`.
//!
//! Runs the *real* pipeline (`jobs::run_job`) end to end against paced, streaming
//! stand-ins, so the run tracker and report rendering can be exercised with **zero
//! network, keys, or cost**. Compiled only under the `demo-run` feature, which is
//! not in `default` and not enabled by `tauri build` — so neither this module nor
//! its mock content ever reaches the shipped binary. Triggered by setting
//! `MARKET_SIGNAL_DEMO=1` in a build that enabled the feature (see `lib.rs`).
//!
//! Each stage here is a thin, ctx-aware decorator: it emits the per-request tracker
//! rows and streams tokens/thinking with small pauses (so the motion — the progress
//! fill, the breathe, the streamed text — is actually watchable), then **delegates
//! to the existing offline stubs** for the real return data. Delegating means the
//! coverage floor still passes and no domain return types are reconstructed here;
//! the only new behavior is emission + pacing. Cancellation is polled throughout so
//! a demo run also exercises the Cancel button.

use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use crate::agent::{
    AnalystAgent, AnalystOutput, MainAgent, MainAgentInput, MainAgentOutput, Posture,
    StubAnalystAgent, StubMainAgent,
};
use crate::cadence::ReportCadence;
use crate::data_sources::{BaselineMarketData, MarketDataSource, StubMarketDataSource};
use crate::headline_filter::{HeadlineCluster, HeadlineFilter, StubHeadlineFilter};
use crate::news::{NewsSource, RawHeadline, StubNewsSource};
use crate::pipeline::{AnalystStages, ResearchStages};
use crate::progress::RunContext;
use crate::research_executor::{SearchBackend, StubSearchBackend};
use crate::research_router::{ResearchPlan, ResearchRouter, RouterInput, StubResearchRouter};

/// Pause between emissions so the tracker animates at a human pace; short-circuits
/// the moment a cancel is requested so the run stops promptly.
fn paced(ctx: &RunContext, ms: u64) {
    if ctx.is_cancelled() {
        return;
    }
    sleep(Duration::from_millis(ms));
}

/// Stream a block of text through `emit` word-by-word with a small per-word pause,
/// bailing on cancel. Used for both the main-agent report/reasoning and the analyst
/// reasoning, so the streamed panes fill the way the live adapters drive them.
fn stream(ctx: &RunContext, text: &str, per_word_ms: u64, mut emit: impl FnMut(&str)) {
    for word in text.split_inclusive(' ') {
        if ctx.is_cancelled() {
            return;
        }
        emit(word);
        sleep(Duration::from_millis(per_word_ms));
    }
}

// --- Baseline scan (Step 3) -------------------------------------------------

/// Representative baseline rows, grouped "baseline" so the tracker files them under
/// the baseline step (mirrors `App.vue`'s request-group routing).
const BASELINE_ROWS: &[(&str, &str)] = &[
    ("FMP", "Index levels (Dow / S&P / Nasdaq)"),
    ("FMP", "VIX & sector performance"),
    ("FRED", "Treasury yields & the curve"),
    ("FRED", "Gross Domestic Product"),
    ("BLS", "Labor series (CPI, unemployment, payrolls, wages)"),
    ("CFTC", "Commitments of Traders positioning"),
];

pub struct DemoMarketDataSource {
    ctx: Arc<RunContext>,
}

impl MarketDataSource for DemoMarketDataSource {
    fn baseline_scan(&self, cadence: ReportCadence) -> anyhow::Result<BaselineMarketData> {
        for (provider, name) in BASELINE_ROWS {
            self.ctx.request_started(*provider, "baseline", *name, *name);
            paced(&self.ctx, 160);
            self.ctx
                .request_finished(*provider, "baseline", *name, *name, "ok", None);
        }
        // Delegate for the data so the coverage floor passes, as in the test runs.
        StubMarketDataSource.baseline_scan(cadence)
    }
}

// --- Research half (Steps 7–11) ---------------------------------------------

/// News-gather rows, including a failed GDELT sweep (the fail-soft case the tracker
/// renders in accent-text) and the supplementary FMP Articles feed.
const NEWS_ROWS: &[(&str, &str, bool)] = &[
    ("TAVILY", "US stock market policy and politics", true),
    ("TAVILY", "geopolitics affecting global markets", true),
    ("TAVILY", "China trade and tariffs", true),
    ("TAVILY", "energy and oil prices", true),
    ("TAVILY", "corporate earnings results", true),
    ("TAVILY", "AI and semiconductors", true),
    ("TAVILY", "US economy inflation and the Federal Reserve", true),
    ("GDELT", "Geopolitical news sweep", false),
    ("FMP", "FMP Articles feed", true),
];

struct DemoNewsSource {
    ctx: Arc<RunContext>,
}

impl NewsSource for DemoNewsSource {
    fn gather(&self, cadence: ReportCadence) -> anyhow::Result<Vec<RawHeadline>> {
        for (provider, name, ok) in NEWS_ROWS {
            self.ctx.request_started(*provider, "news", *name, *name);
            paced(&self.ctx, 200);
            if *ok {
                self.ctx
                    .request_finished(*provider, "news", *name, *name, "ok", None);
            } else {
                self.ctx.request_finished(
                    *provider,
                    "news",
                    *name,
                    *name,
                    "failed",
                    Some("rate-limited (HTTP 429) — degraded fail-soft".to_string()),
                );
            }
        }
        StubNewsSource.gather(cadence)
    }
}

struct DemoHeadlineFilter {
    ctx: Arc<RunContext>,
}

impl HeadlineFilter for DemoHeadlineFilter {
    fn filter(
        &self,
        headlines: Vec<RawHeadline>,
        report_date: Option<&str>,
    ) -> anyhow::Result<Vec<HeadlineCluster>> {
        self.ctx
            .request_started("OPENAI", "filter", "headline-filter", "Headline filtering");
        paced(&self.ctx, 350);
        self.ctx.request_finished(
            "OPENAI",
            "filter",
            "headline-filter",
            "Headline filtering",
            "ok",
            None,
        );
        StubHeadlineFilter.filter(headlines, report_date)
    }
}

struct DemoResearchRouter {
    ctx: Arc<RunContext>,
}

impl ResearchRouter for DemoResearchRouter {
    fn route(&self, input: RouterInput) -> anyhow::Result<ResearchPlan> {
        self.ctx
            .request_started("ANTHROPIC", "routing", "research-router", "Research routing");
        paced(&self.ctx, 400);
        self.ctx.request_finished(
            "ANTHROPIC",
            "routing",
            "research-router",
            "Research routing",
            "ok",
            None,
        );
        StubResearchRouter.route(input)
    }
}

struct DemoSearchBackend {
    ctx: Arc<RunContext>,
}

impl SearchBackend for DemoSearchBackend {
    fn search(&self, query: &str) -> anyhow::Result<Vec<RawHeadline>> {
        self.ctx
            .request_started("TAVILY", "research", query, query);
        paced(&self.ctx, 350);
        self.ctx
            .request_finished("TAVILY", "research", query, query, "ok", None);
        StubSearchBackend.search(query)
    }
}

// --- Analysts (Steps 12–15) -------------------------------------------------

fn analyst_thinking(posture: Posture) -> &'static str {
    match posture {
        Posture::Bull => "Breadth is broadening — the equal-weight index is finally tracking the cap-weighted one. If earnings revisions hold positive and the Fed signals one more cut, the late-cycle melt-up case strengthens. Weighting the AI-capex durability lens here.",
        Posture::Bear => "The curve's dis-inversion historically precedes, not relieves, the slowdown. Credit spreads look complacent and positioning is one-sided. I read the rate-path headline as a regime-change tell I don't want to discount.",
        Posture::Balanced => "Both reads hold on different horizons: risk-on tactically, late-cycle structurally. The honest framing names a falsifiable trigger — the next two payroll prints against the breadth signal — rather than splitting the difference.",
    }
}

struct DemoAnalystAgent {
    posture: Posture,
    ctx: Arc<RunContext>,
}

impl AnalystAgent for DemoAnalystAgent {
    fn review(
        &self,
        packet: &crate::research_packet::ResearchPacket,
        cadence: ReportCadence,
    ) -> anyhow::Result<AnalystOutput> {
        let posture = self.posture;
        stream(&self.ctx, analyst_thinking(posture), 22, |w| {
            self.ctx.analyst_thinking(posture.as_str(), w)
        });
        StubAnalystAgent::new(posture).review(packet, cadence)
    }
}

// --- Main agent (Step 16) ---------------------------------------------------

const MAIN_THINKING: &str = "Reconciling the three reviews. The Bull and Bear disagree on horizon, not facts — I'll anchor the thesis on the orthogonal axes (risk posture vs. cycle) so the report holds both without hedging into mush. Lead with the breadth shift; flag the rate-path headline as the key unresolved question.";

pub struct DemoMainAgent {
    ctx: Arc<RunContext>,
}

impl MainAgent for DemoMainAgent {
    fn generate(&self, input: MainAgentInput) -> anyhow::Result<MainAgentOutput> {
        // Stream the extended-thinking pane first, then the report body, mirroring how
        // the live adapter drives the two channels.
        stream(&self.ctx, MAIN_THINKING, 16, |w| self.ctx.agent_thinking(w));
        let out = StubMainAgent.generate(input)?;
        stream(&self.ctx, &out.markdown, 11, |w| self.ctx.agent_token(w));
        Ok(out)
    }
}

// --- Bundle constructors (called from the gated command path in lib.rs) -----

pub fn main_agent(ctx: Arc<RunContext>) -> DemoMainAgent {
    DemoMainAgent { ctx }
}

pub fn market_data(ctx: Arc<RunContext>) -> DemoMarketDataSource {
    DemoMarketDataSource { ctx }
}

pub fn research_stages(ctx: Arc<RunContext>) -> ResearchStages {
    ResearchStages {
        news: Box::new(DemoNewsSource { ctx: ctx.clone() }),
        filter: Box::new(DemoHeadlineFilter { ctx: ctx.clone() }),
        router: Box::new(DemoResearchRouter { ctx: ctx.clone() }),
        search: Box::new(DemoSearchBackend { ctx }),
    }
}

pub fn analyst_stages(ctx: Arc<RunContext>) -> AnalystStages {
    AnalystStages {
        bull: Box::new(DemoAnalystAgent {
            posture: Posture::Bull,
            ctx: ctx.clone(),
        }),
        bear: Box::new(DemoAnalystAgent {
            posture: Posture::Bear,
            ctx: ctx.clone(),
        }),
        balanced: Box::new(DemoAnalystAgent {
            posture: Posture::Balanced,
            ctx,
        }),
    }
}
