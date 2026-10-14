//! Run-progress reporting and cooperative cancellation for a single run, kept
//! free of any `tauri` dependency: the live `emit`-backed reporter lives in
//! `lib.rs`, tests use [`NoopReporter`]. Cancellation is polled at step and
//! request boundaries; a blocking call already in flight is not interrupted.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

/// One progress event, always carried inside a [`ProgressMessage`]. The
/// string-valued `status` fields carry a small fixed vocabulary rather than a
/// Rust enum so this module keeps no dependency on `data_sources` (`GapReason`)
/// or `jobs` (`JobState`); the vocabularies are documented per variant.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProgressEvent {
    /// The run has begun; `label` is a short human title for the tracker header.
    RunStarted { label: String },
    /// A pipeline step has started. `step` is a stable key the UI keys rows on;
    /// `label` is its human title.
    StepStarted { step: String, label: String },
    /// A pipeline step finished. `status` ∈ {`ok`, `failed`, `cancelled`};
    /// `detail` carries a one-line reason when not `ok`.
    StepFinished {
        step: String,
        status: String,
        detail: Option<String>,
    },
    /// One HTTP call is about to be made. Emitted only when a request is
    /// actually made, so tracker rows stay one-to-one with network calls.
    /// `series_id` keys the matching [`Self::RequestFinished`]. `step` is
    /// stamped by [`RunContext::emit`] from the run's active step, never
    /// supplied by the caller; `None` means no step was open and the tracker
    /// renders the row unattributed.
    RequestStarted {
        provider: String,
        group: String,
        series_id: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<String>,
        /// What a research request asked for — see [`RequestTarget`]. Absent
        /// on every other row.
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<RequestTarget>,
    },
    /// A request resolved — paired with its `RequestStarted` by
    /// `provider`/`group`/`series_id`. `status` is `ok`, the `GapReason` kebab
    /// label (`unavailable` / `rejected` / `malformed` / `out-of-scope`) when it
    /// degraded to a gap, or `empty` for a 2xx that carried no usable data and
    /// recorded no gap. `step` is stamped as on [`Self::RequestStarted`].
    RequestFinished {
        provider: String,
        group: String,
        series_id: String,
        name: String,
        status: String,
        detail: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<String>,
        /// The same target the row's [`Self::RequestStarted`] carried, so a
        /// row resolved without its start still names its subject.
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<RequestTarget>,
    },
    /// A coalesced chunk of the main agent's streamed report text.
    AgentToken { delta: String },
    /// A coalesced chunk of the main agent's streamed reasoning. A model that
    /// surfaces no thinking never sends it, so the tracker shows no reasoning
    /// rather than an error.
    AgentThinking { delta: String },
    /// A coalesced chunk of one analyst's streamed reasoning, `posture` ∈
    /// {`bull`, `bear`, `balanced`}. The analyst stage streams thoughts only;
    /// the structured review body never streams.
    AnalystThinking { posture: String, delta: String },
    /// A streaming agent call is about to re-attempt under its bounded retry
    /// (`docs/agents.md §Bounded Retry on the Streaming Calls`), so the tracker
    /// clears that call's live panes first: the main agent's when `posture` is
    /// absent, otherwise the named analyst's.
    StreamRestarted {
        #[serde(skip_serializing_if = "Option::is_none")]
        posture: Option<String>,
    },
    /// A coalesced chunk of streamed reasoning scoped to the tracker step
    /// `step`, unlike [`Self::AgentThinking`] / [`Self::AnalystThinking`],
    /// which route to the report workflow's fixed steps.
    StepThinking { step: String, delta: String },
    /// A local-model chat call is being issued — a diagnostic boundary the
    /// thought-log sink and the stderr tee consume; the tracker does not render
    /// it (`docs/run-tracking.md §Thought-log capture`). `call` is the run's
    /// per-call counter (1-based, monotonic); the rest are the request's own
    /// controls, never prompt text. `step` is stamped as on
    /// [`Self::RequestStarted`].
    ModelCallStarted {
        call: u64,
        stage: String,
        model: String,
        /// The request's `think` flag as sent: `Some(true)` / `Some(false)`, or
        /// `None` when the request left the model's own default in force.
        #[serde(skip_serializing_if = "Option::is_none")]
        think: Option<bool>,
        streamed: bool,
        tools: bool,
        format: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        num_ctx: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        num_predict: Option<u32>,
        prompt_chars: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<String>,
    },
    /// The local-model call `call` resolved. `status` ∈ {`ok`, `failed`};
    /// `detail` is a failed call's capped top-level message.
    ModelCallFinished {
        call: u64,
        stage: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        usage: crate::local_model::PromptUsage,
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<String>,
    },
    /// The run reached a terminal state. `status` ∈ {`successful`, `failed`,
    /// `cancelled`}; `report_id` is set only on success.
    RunFinished {
        status: String,
        detail: Option<String>,
        report_id: Option<String>,
    },
}

/// What a research request asked for — the search query or the page address
/// (`docs/run-tracking.md §What the Tracker Shows`). `kind` is `search` or
/// `fetch`. The series id stays the pairing key; the tracker never parses a
/// subject out of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequestTarget {
    pub kind: String,
    pub text: String,
}

/// The wire payload handed to a [`ProgressReporter`]. `seq` lets the frontend
/// order or dedupe events; `run_id` lets it discard a straggler from a prior
/// run.
#[derive(Debug, Clone, Serialize)]
pub struct ProgressMessage {
    pub run_id: String,
    pub seq: u64,
    #[serde(flatten)]
    pub event: ProgressEvent,
}

/// What a local-model call boundary reports about its request: the caller's
/// stage label and the request's controls, never its prompt text.
#[derive(Debug, Clone)]
pub struct ModelCallInfo {
    pub stage: String,
    pub model: String,
    pub think: Option<bool>,
    pub streamed: bool,
    pub tools: bool,
    pub format: bool,
    pub num_ctx: Option<u32>,
    pub num_predict: Option<u32>,
    pub prompt_chars: u64,
}

/// How a local-model call resolved; a failure carries its capped top-level
/// message in `detail`.
#[derive(Debug, Clone)]
pub struct ModelCallOutcome {
    pub ok: bool,
    pub detail: Option<String>,
    pub usage: crate::local_model::PromptUsage,
}

/// Sink for [`ProgressMessage`]s. `Send + Sync` so a `RunContext` can cross the
/// `spawn_blocking` boundary and be held by the adapters.
pub trait ProgressReporter: Send + Sync {
    fn report(&self, message: &ProgressMessage);
}

/// One stderr line per run / step / request / model-call event — the durable
/// trace a failed run leaves even when nothing persists and no window is
/// listening. Token and thinking deltas are deliberately skipped.
fn tee_to_stderr(run_id: &str, event: &ProgressEvent) {
    match event {
        ProgressEvent::RunStarted { label } => {
            eprintln!("[run {run_id}] started: {label}");
        }
        ProgressEvent::StepStarted { step, label } => {
            eprintln!("[run {run_id}] step {step}: started ({label})");
        }
        ProgressEvent::StepFinished { step, status, detail } => match detail {
            Some(d) => eprintln!("[run {run_id}] step {step}: {status} — {d}"),
            None => eprintln!("[run {run_id}] step {step}: {status}"),
        },
        ProgressEvent::RequestStarted { provider, group, series_id, step, .. } => {
            let step = step.as_deref().unwrap_or("unattributed");
            eprintln!("[run {run_id}] [{step}] {provider} {group}/{series_id}: sent");
        }
        ProgressEvent::RequestFinished { provider, group, series_id, status, detail, step, .. } => {
            let step = step.as_deref().unwrap_or("unattributed");
            match detail {
                Some(d) => eprintln!("[run {run_id}] [{step}] {provider} {group}/{series_id}: {status} — {d}"),
                None => eprintln!("[run {run_id}] [{step}] {provider} {group}/{series_id}: {status}"),
            }
        }
        ProgressEvent::ModelCallStarted { call, stage, model, think, streamed, step, .. } => {
            let step = step.as_deref().unwrap_or("unattributed");
            let think = match think {
                Some(true) => "think on",
                Some(false) => "think off",
                None => "think default",
            };
            let transport = if *streamed { "streamed" } else { "non-streaming" };
            eprintln!("[run {run_id}] [{step}] model call {call}: {stage} → {model} ({think}, {transport})");
        }
        ProgressEvent::ModelCallFinished {
            call,
            stage,
            status,
            detail,
            usage,
            step,
            ..
        } => {
            let step = step.as_deref().unwrap_or("unattributed");
            let elapsed = elapsed_label(usage.elapsed_ms);
            match detail {
                Some(d) => eprintln!("[run {run_id}] [{step}] model call {call}: {stage} {status} after {elapsed} — {d}"),
                None => eprintln!("[run {run_id}] [{step}] model call {call}: {stage} {status} after {elapsed}"),
            }
        }
        ProgressEvent::RunFinished { status, detail, .. } => match detail {
            Some(d) => eprintln!("[run {run_id}] finished: {status} — {d}"),
            None => eprintln!("[run {run_id}] finished: {status}"),
        },
        ProgressEvent::StreamRestarted { posture } => match posture {
            Some(p) => eprintln!("[run {run_id}] stream restarted: analyst {p}"),
            None => eprintln!("[run {run_id}] stream restarted: main agent"),
        },
        ProgressEvent::AgentToken { .. }
        | ProgressEvent::AgentThinking { .. }
        | ProgressEvent::AnalystThinking { .. }
        | ProgressEvent::StepThinking { .. } => {}
    }
}

/// A human elapsed-time label — `900ms`, `12s`, `7m41s`, `1h02m05s` — shared
/// by the stderr tee and the thought-log fences so the two never disagree.
pub fn elapsed_label(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    let secs = ms / 1_000;
    let (h, m, s) = (secs / 3_600, (secs % 3_600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// The default reporter for tests and offline smokes.
pub struct NoopReporter;

impl ProgressReporter for NoopReporter {
    fn report(&self, _message: &ProgressMessage) {}
}

/// The per-run context: who to report to, whether a cancel has been requested,
/// and the run's identity. Shared by `Arc` with the adapters.
pub struct RunContext {
    run_id: String,
    reporter: Arc<dyn ProgressReporter>,
    /// Shared with the Tauri layer's managed cancel flag, so `cancel_run` flips
    /// the same bool this run polls. A relaxed load is enough — a cooperative
    /// checkpoint, not a synchronization point.
    cancel: Arc<AtomicBool>,
    seq: AtomicU64,
    /// The step most recently started and not yet finished; [`Self::emit`]
    /// stamps every request event with it. A single cell, not a stack: the
    /// pipeline's only concurrency (the analyst trio) lives inside one step. A
    /// stage wanting two concurrently-open request-owning steps must extend
    /// this seam; until then an unowned request degrades to an unattributed
    /// row, never a misattributed one.
    active_step: Mutex<Option<String>>,
    /// The `call` number both boundary events of one local-model call share.
    calls: AtomicU64,
}

impl RunContext {
    pub fn new(
        run_id: impl Into<String>,
        reporter: Arc<dyn ProgressReporter>,
        cancel: Arc<AtomicBool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            run_id: run_id.into(),
            reporter,
            cancel,
            seq: AtomicU64::new(0),
            active_step: Mutex::new(None),
            calls: AtomicU64::new(0),
        })
    }

    /// A context that reports nowhere and is never cancelled.
    pub fn noop() -> Arc<Self> {
        Self::new(
            "noop",
            Arc::new(NoopReporter),
            Arc::new(AtomicBool::new(false)),
        )
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Polled at step and request boundaries; an in-flight HTTP call is not
    /// interrupted.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Called once the run owns the concurrency slot (not when the context is
    /// built), so a competing attempt that is then skipped can't reset an
    /// already-active run's cancellation.
    pub fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// The single choke point every helper routes through, and the stderr
    /// tee's one home. Request and model-call events are stamped here with the
    /// run's active step, so the adapters emitting them never need to know
    /// which step is running.
    fn emit(&self, mut event: ProgressEvent) {
        if let ProgressEvent::RequestStarted { step, .. }
        | ProgressEvent::RequestFinished { step, .. }
        | ProgressEvent::ModelCallStarted { step, .. }
        | ProgressEvent::ModelCallFinished { step, .. } = &mut event
        {
            *step = self.active_step.lock().unwrap().clone();
        }
        tee_to_stderr(&self.run_id, &event);
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        self.reporter.report(&ProgressMessage {
            run_id: self.run_id.clone(),
            seq,
            event,
        });
    }

    pub fn run_started(&self, label: impl Into<String>) {
        self.emit(ProgressEvent::RunStarted {
            label: label.into(),
        });
    }

    /// Also makes the step the run's active step until the matching
    /// [`Self::step_finished`].
    pub fn step_started(&self, step: impl Into<String>, label: impl Into<String>) {
        let step = step.into();
        *self.active_step.lock().unwrap() = Some(step.clone());
        self.emit(ProgressEvent::StepStarted {
            step,
            label: label.into(),
        });
    }

    /// Clears the active step only when the key matches the step currently
    /// owning it, so a stray or out-of-order close can't steal ownership from
    /// a step that already superseded it.
    pub fn step_finished(
        &self,
        step: impl Into<String>,
        status: impl Into<String>,
        detail: Option<String>,
    ) {
        let step = step.into();
        {
            let mut active = self.active_step.lock().unwrap();
            if active.as_deref() == Some(step.as_str()) {
                *active = None;
            }
        }
        self.emit(ProgressEvent::StepFinished {
            step,
            status: status.into(),
            detail,
        });
    }

    pub fn request_started(
        &self,
        provider: impl Into<String>,
        group: impl Into<String>,
        series_id: impl Into<String>,
        name: impl Into<String>,
    ) {
        self.emit(ProgressEvent::RequestStarted {
            provider: provider.into(),
            group: group.into(),
            series_id: series_id.into(),
            name: name.into(),
            // Placeholder — `emit` overwrites it with the run's active step.
            step: None,
            target: None,
        });
    }

    /// [`Self::request_started`] for a research request, naming what it asked for.
    pub fn request_started_with_target(
        &self,
        provider: impl Into<String>,
        group: impl Into<String>,
        series_id: impl Into<String>,
        name: impl Into<String>,
        target: RequestTarget,
    ) {
        self.emit(ProgressEvent::RequestStarted {
            provider: provider.into(),
            group: group.into(),
            series_id: series_id.into(),
            name: name.into(),
            step: None,
            target: Some(target),
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn request_finished(
        &self,
        provider: impl Into<String>,
        group: impl Into<String>,
        series_id: impl Into<String>,
        name: impl Into<String>,
        status: impl Into<String>,
        detail: Option<String>,
    ) {
        self.emit(ProgressEvent::RequestFinished {
            provider: provider.into(),
            group: group.into(),
            series_id: series_id.into(),
            name: name.into(),
            status: status.into(),
            detail,
            // Placeholder — `emit` overwrites it with the run's active step.
            step: None,
            target: None,
        });
    }

    /// [`Self::request_finished`] for a research request, carrying the same
    /// target its start did.
    #[allow(clippy::too_many_arguments)]
    pub fn request_finished_with_target(
        &self,
        provider: impl Into<String>,
        group: impl Into<String>,
        series_id: impl Into<String>,
        name: impl Into<String>,
        status: impl Into<String>,
        detail: Option<String>,
        target: RequestTarget,
    ) {
        self.emit(ProgressEvent::RequestFinished {
            provider: provider.into(),
            group: group.into(),
            series_id: series_id.into(),
            name: name.into(),
            status: status.into(),
            detail,
            step: None,
            target: Some(target),
        });
    }

    pub fn agent_token(&self, delta: impl Into<String>) {
        self.emit(ProgressEvent::AgentToken {
            delta: delta.into(),
        });
    }

    pub fn agent_thinking(&self, delta: impl Into<String>) {
        self.emit(ProgressEvent::AgentThinking {
            delta: delta.into(),
        });
    }

    pub fn analyst_thinking(&self, posture: impl Into<String>, delta: impl Into<String>) {
        self.emit(ProgressEvent::AnalystThinking {
            posture: posture.into(),
            delta: delta.into(),
        });
    }

    /// `None` for the main agent, the posture for an analyst.
    pub fn stream_restarted(&self, posture: Option<&str>) {
        self.emit(ProgressEvent::StreamRestarted {
            posture: posture.map(str::to_string),
        });
    }

    pub fn step_thinking(&self, step: impl Into<String>, delta: impl Into<String>) {
        self.emit(ProgressEvent::StepThinking {
            step: step.into(),
            delta: delta.into(),
        });
    }

    /// Returns the call number the matching [`Self::model_call_finished`] must
    /// carry.
    pub fn model_call_started(&self, info: ModelCallInfo) -> u64 {
        let call = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        self.emit(ProgressEvent::ModelCallStarted {
            call,
            stage: info.stage,
            model: info.model,
            think: info.think,
            streamed: info.streamed,
            tools: info.tools,
            format: info.format,
            num_ctx: info.num_ctx,
            num_predict: info.num_predict,
            prompt_chars: info.prompt_chars,
            // Placeholder — `emit` overwrites it with the run's active step.
            step: None,
        });
        call
    }

    pub fn model_call_finished(&self, call: u64, stage: impl Into<String>, outcome: ModelCallOutcome) {
        self.emit(ProgressEvent::ModelCallFinished {
            call,
            stage: stage.into(),
            status: if outcome.ok { "ok" } else { "failed" }.to_string(),
            detail: outcome.detail,
            usage: outcome.usage,
            // Placeholder — `emit` overwrites it with the run's active step.
            step: None,
        });
    }

    pub fn run_finished(
        &self,
        status: impl Into<String>,
        detail: Option<String>,
        report_id: Option<String>,
    ) {
        self.emit(ProgressEvent::RunFinished {
            status: status.into(),
            detail,
            report_id,
        });
    }
}

/// Records every message, for tests that assert on the emitted stream.
#[cfg(test)]
#[derive(Default)]
pub struct RecordingReporter(std::sync::Mutex<Vec<ProgressMessage>>);

#[cfg(test)]
impl RecordingReporter {
    pub fn messages(&self) -> Vec<ProgressMessage> {
        self.0.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl ProgressReporter for RecordingReporter {
    fn report(&self, message: &ProgressMessage) {
        self.0.lock().unwrap().push(message.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_carry_a_monotonic_seq_and_the_run_id() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-1", rec.clone(), Arc::new(AtomicBool::new(false)));
        ctx.run_started("Market Signal report");
        ctx.step_started("baseline", "Baseline scan");
        ctx.step_finished("baseline", "ok", None);

        let msgs = rec.messages();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].seq, 0);
        assert_eq!(msgs[1].seq, 1);
        assert_eq!(msgs[2].seq, 2);
        assert!(msgs.iter().all(|m| m.run_id == "run-1"));
    }

    #[test]
    fn event_serializes_with_a_kebab_kind_tag_and_flattened_fields() {
        let msg = ProgressMessage {
            run_id: "r".into(),
            seq: 7,
            event: ProgressEvent::RequestFinished {
                provider: "FRED".into(),
                group: "macro-levels".into(),
                series_id: "DGS10".into(),
                name: "10-Year Treasury".into(),
                status: "ok".into(),
                detail: None,
                step: Some("baseline".into()),
                target: None,
            },
        };
        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["kind"], "request-finished");
        assert_eq!(v["run_id"], "r");
        assert_eq!(v["seq"], 7);
        assert_eq!(v["provider"], "FRED");
        assert_eq!(v["group"], "macro-levels");
        assert_eq!(v["series_id"], "DGS10");
        assert_eq!(v["name"], "10-Year Treasury");
        assert_eq!(v["status"], "ok");
        assert_eq!(v["step"], "baseline");
        // Absent, not null: the frontend's "has a subject" check is an absent-field test.
        assert!(v.get("target").is_none());
    }

    #[test]
    fn a_targeted_request_serializes_its_subject_as_a_nested_object() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-t", rec.clone(), Arc::new(AtomicBool::new(false)));
        ctx.request_started_with_target(
            "web",
            "research",
            "search: widget co earnings",
            "competitive-position",
            RequestTarget { kind: "search".into(), text: "widget co earnings".into() },
        );
        ctx.request_finished_with_target(
            "web",
            "research",
            "search: widget co earnings",
            "competitive-position",
            "ok",
            Some("3 hits".into()),
            RequestTarget { kind: "search".into(), text: "widget co earnings".into() },
        );
        let msgs = rec.messages();
        assert_eq!(msgs.len(), 2);
        for m in &msgs {
            let v = serde_json::to_value(m).unwrap();
            assert_eq!(v["target"]["kind"], "search");
            assert_eq!(v["target"]["text"], "widget co earnings");
            assert_eq!(v["series_id"], "search: widget co earnings");
            assert_eq!(v["name"], "competitive-position");
        }
    }

    #[test]
    fn an_unowned_request_serializes_with_no_step_field_at_all() {
        // Absent, not null: the frontend's "no step" check is an absent-field test.
        let msg = ProgressMessage {
            run_id: "r".into(),
            seq: 0,
            event: ProgressEvent::RequestStarted {
                provider: "FMP".into(),
                group: "indices".into(),
                series_id: "SPX".into(),
                name: "S&P 500".into(),
                step: None,
                target: None,
            },
        };
        let v = serde_json::to_value(&msg).unwrap();
        assert!(v.get("step").is_none());
    }

    #[test]
    fn requests_are_stamped_with_the_active_step() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-5", rec.clone(), Arc::new(AtomicBool::new(false)));

        ctx.request_started("FMP", "indices", "SPX", "S&P 500");
        ctx.step_started("baseline", "Baseline scan");
        ctx.request_started("FRED", "macro-levels", "DGS10", "10-Year Treasury");
        ctx.request_finished("FRED", "macro-levels", "DGS10", "10-Year Treasury", "ok", None);
        ctx.step_finished("baseline", "ok", None);
        ctx.request_finished("FMP", "indices", "SPX", "S&P 500", "ok", None);

        let steps: Vec<Option<String>> = rec
            .messages()
            .iter()
            .filter_map(|m| match &m.event {
                ProgressEvent::RequestStarted { step, .. }
                | ProgressEvent::RequestFinished { step, .. } => Some(step.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            steps,
            vec![
                None,
                Some("baseline".to_string()),
                Some("baseline".to_string()),
                None,
            ]
        );
    }

    #[test]
    fn a_stray_close_cannot_steal_ownership_from_a_superseding_step() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-6", rec.clone(), Arc::new(AtomicBool::new(false)));

        ctx.step_started("rates", "Load rate anchors");
        ctx.step_started("holding-AAPL", "Analyze AAPL");
        ctx.step_finished("rates", "ok", None);
        ctx.request_started("FMP", "company-quote", "AAPL", "AAPL quote");

        let last = rec.messages().pop().unwrap();
        match last.event {
            ProgressEvent::RequestStarted { step, .. } => {
                assert_eq!(step.as_deref(), Some("holding-AAPL"));
            }
            other => panic!("expected RequestStarted, got {other:?}"),
        }
    }

    #[test]
    fn concurrent_requests_inside_one_step_all_carry_its_stamp() {
        // The analyst-trio shape: scoped threads sharing one context.
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-7", rec.clone(), Arc::new(AtomicBool::new(false)));

        ctx.step_started("analysts", "Running the analyst agents");
        std::thread::scope(|scope| {
            for posture in ["bull", "bear", "balanced"] {
                let ctx = &ctx;
                scope.spawn(move || {
                    ctx.request_started("OpenAI", "analyst", posture, posture);
                    ctx.request_finished("OpenAI", "analyst", posture, posture, "ok", None);
                });
            }
        });
        ctx.step_finished("analysts", "ok", None);

        let request_steps: Vec<Option<String>> = rec
            .messages()
            .iter()
            .filter_map(|m| match &m.event {
                ProgressEvent::RequestStarted { step, .. }
                | ProgressEvent::RequestFinished { step, .. } => Some(step.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(request_steps.len(), 6);
        assert!(request_steps
            .iter()
            .all(|s| s.as_deref() == Some("analysts")));
    }

    #[test]
    fn agent_thinking_serializes_as_a_kebab_kind_with_a_delta() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-2", rec.clone(), Arc::new(AtomicBool::new(false)));
        ctx.agent_thinking("Weighing the bull case");

        let msgs = rec.messages();
        assert_eq!(msgs.len(), 1);
        let v = serde_json::to_value(&msgs[0]).unwrap();
        assert_eq!(v["kind"], "agent-thinking");
        assert_eq!(v["delta"], "Weighing the bull case");
        assert_eq!(v["run_id"], "run-2");
    }

    #[test]
    fn analyst_thinking_serializes_with_a_posture_tag() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-3", rec.clone(), Arc::new(AtomicBool::new(false)));
        ctx.analyst_thinking("bear", "The curve is lying about the cycle");

        let msgs = rec.messages();
        assert_eq!(msgs.len(), 1);
        let v = serde_json::to_value(&msgs[0]).unwrap();
        assert_eq!(v["kind"], "analyst-thinking");
        assert_eq!(v["posture"], "bear");
        assert_eq!(v["delta"], "The curve is lying about the cycle");
        assert_eq!(v["run_id"], "run-3");
    }

    #[test]
    fn stream_restarted_names_an_analyst_posture_and_omits_it_for_the_main_agent() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-5", rec.clone(), Arc::new(AtomicBool::new(false)));
        ctx.stream_restarted(None);
        ctx.stream_restarted(Some("bull"));

        let msgs = rec.messages();
        assert_eq!(msgs.len(), 2);
        let main = serde_json::to_value(&msgs[0]).unwrap();
        assert_eq!(main["kind"], "stream-restarted");
        assert!(main.get("posture").is_none(), "{main}");
        let analyst = serde_json::to_value(&msgs[1]).unwrap();
        assert_eq!(analyst["kind"], "stream-restarted");
        assert_eq!(analyst["posture"], "bull");
    }

    #[test]
    fn step_thinking_serializes_with_its_step_key() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-4", rec.clone(), Arc::new(AtomicBool::new(false)));
        ctx.step_thinking("holding-AAPL", "Weighing the trim against the tilt");

        let msgs = rec.messages();
        assert_eq!(msgs.len(), 1);
        let v = serde_json::to_value(&msgs[0]).unwrap();
        assert_eq!(v["kind"], "step-thinking");
        assert_eq!(v["step"], "holding-AAPL");
        assert_eq!(v["delta"], "Weighing the trim against the tilt");
        assert_eq!(v["run_id"], "run-4");
    }

    fn call_info(stage: &str) -> ModelCallInfo {
        ModelCallInfo {
            stage: stage.into(),
            model: "qwen".into(),
            think: Some(true),
            streamed: false,
            tools: true,
            format: false,
            num_ctx: Some(131_072),
            num_predict: Some(65_536),
            prompt_chars: 4_200,
        }
    }

    #[test]
    fn model_call_boundaries_number_calls_and_carry_the_active_step() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-8", rec.clone(), Arc::new(AtomicBool::new(false)));

        let first = ctx.model_call_started(call_info("probe"));
        ctx.step_started("holding-AAPL", "Analyze AAPL");
        let second = ctx.model_call_started(call_info("interpret AAPL"));
        ctx.model_call_finished(
            second,
            "interpret AAPL",
            ModelCallOutcome {
                ok: true,
                detail: None,
                usage: crate::local_model::PromptUsage {
                    elapsed_ms: 461_000,
                    api: crate::local_model::ApiCounters {
                        prompt_eval_count: Some(41_203),
                        eval_count: Some(8_921),
                        ..Default::default()
                    },
                    done_reason: Some("stop".into()),
                    ..Default::default()
                },
            },
        );
        ctx.step_finished("holding-AAPL", "ok", None);
        ctx.model_call_finished(
            first,
            "probe",
            ModelCallOutcome {
                ok: false,
                detail: Some("local model returned 500".into()),
                usage: crate::local_model::PromptUsage {
                    elapsed_ms: 12,
                    api: crate::local_model::ApiCounters {
                        prompt_eval_count: None,
                        eval_count: None,
                        ..Default::default()
                    },
                    done_reason: None,
                    ..Default::default()
                },
            },
        );

        assert_eq!((first, second), (1, 2), "calls number from 1, monotonic");
        let calls: Vec<(u64, Option<String>)> = rec
            .messages()
            .iter()
            .filter_map(|m| match &m.event {
                ProgressEvent::ModelCallStarted { call, step, .. }
                | ProgressEvent::ModelCallFinished { call, step, .. } => {
                    Some((*call, step.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            calls,
            vec![
                (1, None),
                (2, Some("holding-AAPL".to_string())),
                (2, Some("holding-AAPL".to_string())),
                (1, None),
            ],
            "each boundary is stamped with the step open when it was emitted"
        );
    }

    #[test]
    fn model_call_events_serialize_with_named_usage_and_optional_controls() {
        let rec = Arc::new(RecordingReporter::default());
        let ctx = RunContext::new("run-9", rec.clone(), Arc::new(AtomicBool::new(false)));
        let mut info = call_info("distill AAPL");
        info.think = None;
        let call = ctx.model_call_started(info);
        ctx.model_call_finished(
            call,
            "distill AAPL",
            ModelCallOutcome {
                ok: false,
                detail: Some("boom".into()),
                usage: crate::local_model::PromptUsage {
                    elapsed_ms: 1_500,
                    api: crate::local_model::ApiCounters {
                        prompt_eval_count: None,
                        eval_count: None,
                        ..Default::default()
                    },
                    done_reason: None,
                    ..Default::default()
                },
            },
        );

        let msgs = rec.messages();
        let started = serde_json::to_value(&msgs[0]).unwrap();
        assert_eq!(started["kind"], "model-call-started");
        assert_eq!(started["call"], 1);
        assert_eq!(started["stage"], "distill AAPL");
        assert_eq!(started["model"], "qwen");
        assert!(started.get("think").is_none(), "a default think flag is absent, not null");
        assert_eq!(started["streamed"], false);
        assert_eq!(started["tools"], true);
        assert_eq!(started["format"], false);
        assert_eq!(started["num_ctx"], 131_072);
        assert_eq!(started["num_predict"], 65_536);
        assert_eq!(started["prompt_chars"], 4_200);
        assert!(started.get("step").is_none());

        let finished = serde_json::to_value(&msgs[1]).unwrap();
        assert_eq!(finished["kind"], "model-call-finished");
        assert_eq!(finished["call"], 1);
        assert_eq!(finished["status"], "failed");
        assert_eq!(finished["detail"], "boom");
        assert_eq!(finished["usage"]["elapsed_ms"], 1_500);
        assert!(finished["usage"]["api"]["prompt_eval_count"].is_null());
        assert!(finished["usage"]["api"]["eval_count"].is_null());
        assert!(finished["usage"]["done_reason"].is_null());
    }

    #[test]
    fn elapsed_labels_read_humanly() {
        assert_eq!(elapsed_label(900), "900ms");
        assert_eq!(elapsed_label(12_000), "12s");
        assert_eq!(elapsed_label(461_000), "7m41s");
        assert_eq!(elapsed_label(3_725_000), "1h02m05s");
    }

    #[test]
    fn cancel_flag_is_observed_through_the_shared_arc() {
        let cancel = Arc::new(AtomicBool::new(false));
        let ctx = RunContext::new("r", Arc::new(NoopReporter), cancel.clone());
        assert!(!ctx.is_cancelled());
        cancel.store(true, Ordering::Relaxed);
        assert!(ctx.is_cancelled());
    }

    #[test]
    fn noop_context_is_never_cancelled() {
        assert!(!RunContext::noop().is_cancelled());
    }
}
