//! Run-progress reporting + cooperative cancellation for a single report run.
//!
//! The pipeline (`pipeline`), the job lifecycle (`jobs`), and the data/agent
//! adapters are all driven free of any Tauri runtime so they stay unit-testable
//! against stubs. This module is the seam that lets a *live* run nonetheless
//! stream its progress to an open window and be cancelled mid-flight, without
//! pulling Tauri into the spine:
//!
//! - [`ProgressReporter`] is a trait the application layer implements. The live
//!   Tauri command supplies an `emit`-backed reporter (defined in `lib.rs`, so
//!   this module keeps no `tauri` dependency); tests — and a scheduled run with
//!   no open window — use [`NoopReporter`].
//! - [`RunContext`] bundles the run id, the reporter, a shared cancel flag, and a
//!   monotonic sequence counter. It is threaded into `generate_report` and held
//!   by the real adapters, so neither the `MarketDataSource` nor the `MainAgent`
//!   trait signature has to change — the context rides on the concrete adapter.
//! - Cancellation is cooperative: [`RunContext::is_cancelled`] is polled at step
//!   and request boundaries. A `reqwest::blocking` call already in flight is not
//!   interrupted; the cancel lands at the next checkpoint.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use serde::Serialize;

/// One progress event streamed to an open window over a run's lifetime. Serde
/// tags the active variant as `kind` (kebab-case) and flattens its fields, so the
/// frontend switches on `payload.kind`. Always carried inside a [`ProgressMessage`]
/// that adds the run id and sequence.
///
/// The string-valued `status` fields carry a small fixed vocabulary rather than a
/// Rust enum to keep this module free of a dependency on `data_sources`
/// (`GapReason`) and `jobs` (`JobState`): the call sites map their typed outcomes
/// to these labels. The vocabularies are documented per field.
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
    /// A single baseline data request is being initiated — one HTTP call the app is
    /// about to make (a series probe, a date probe, or a batched call). Emitted
    /// *only* when a request is actually made (a short-circuited series sends none),
    /// so tracker rows stay one-to-one with network calls, and the row shows in-flight
    /// before its outcome lands. `series_id` keys the matching [`Self::RequestFinished`].
    RequestStarted {
        provider: String,
        group: String,
        series_id: String,
        name: String,
    },
    /// A single baseline data request resolved — paired with a prior `RequestStarted`
    /// by `provider`/`group`/`series_id`. `status` is `ok` for a resolved value, the
    /// `GapReason` kebab label (`unavailable` / `rejected` / `malformed` /
    /// `out-of-scope`) when it degraded to a gap, or `empty` for a 2xx that carried no
    /// usable data and recorded no gap (e.g. an additive enrichment skipped silently).
    RequestFinished {
        provider: String,
        group: String,
        series_id: String,
        name: String,
        status: String,
        detail: Option<String>,
    },
    /// A coalesced chunk of the main agent's streamed output (decoded report
    /// text), appended to the tracker's live console as the model writes.
    AgentToken { delta: String },
    /// The run reached a terminal state. `status` ∈ {`successful`, `failed`,
    /// `cancelled`}; `report_id` is set only on success.
    RunFinished {
        status: String,
        detail: Option<String>,
        report_id: Option<String>,
    },
}

/// The wire payload actually handed to a [`ProgressReporter`]: a [`ProgressEvent`]
/// stamped with its run id and a per-run monotonic sequence. `seq` lets the
/// frontend order or dedupe events even though Tauri already delivers them in
/// emit order, and `run_id` lets it discard a straggler from a prior run.
#[derive(Debug, Clone, Serialize)]
pub struct ProgressMessage {
    pub run_id: String,
    pub seq: u64,
    #[serde(flatten)]
    pub event: ProgressEvent,
}

/// Sink for [`ProgressMessage`]s. Implemented by the Tauri layer (an `emit`-backed
/// reporter in `lib.rs`) for a live run, and by [`NoopReporter`] everywhere else.
/// `Send + Sync` so a `RunContext` can be shared across the `spawn_blocking`
/// boundary and held by the adapters.
pub trait ProgressReporter: Send + Sync {
    fn report(&self, message: &ProgressMessage);
}

/// Drops every event. The default reporter for tests, offline smokes, and a
/// scheduled run with no window to stream to.
pub struct NoopReporter;

impl ProgressReporter for NoopReporter {
    fn report(&self, _message: &ProgressMessage) {}
}

/// The per-run context threaded through the application layer: who to report to,
/// whether a cancel has been requested, and the run's identity. Constructed once
/// per run in the Tauri command (or as [`RunContext::noop`] in tests), shared by
/// `Arc` with the adapters, and borrowed by `generate_report`.
pub struct RunContext {
    run_id: String,
    reporter: Arc<dyn ProgressReporter>,
    /// Shared with the Tauri layer's managed cancel flag, so the `cancel_run`
    /// command flips the same bool this run polls. A relaxed load is enough — it
    /// is a cooperative checkpoint, not a synchronization point.
    cancel: Arc<AtomicBool>,
    seq: AtomicU64,
}

impl RunContext {
    /// Build a context for a live run. Returns an `Arc` because both the adapters
    /// (which keep a clone) and `generate_report` (which borrows it) share it.
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
        })
    }

    /// A context that reports nowhere and is never cancelled — the default the
    /// real adapters fall back to and what tests / offline smokes pass to
    /// `generate_report`.
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

    /// Whether a cancel has been requested. Polled at step and request
    /// boundaries; an in-flight HTTP call is not interrupted.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Clear the cancel flag for the start of a run. Called once the run owns the
    /// concurrency slot (not when the context is built), so a competing attempt that
    /// is then skipped can't reset an already-active run's cancellation.
    pub fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::Relaxed);
    }

    /// Stamp an event with the run id and the next sequence, then hand it to the
    /// reporter. The single choke point every helper below routes through.
    fn emit(&self, event: ProgressEvent) {
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

    pub fn step_started(&self, step: impl Into<String>, label: impl Into<String>) {
        self.emit(ProgressEvent::StepStarted {
            step: step.into(),
            label: label.into(),
        });
    }

    pub fn step_finished(
        &self,
        step: impl Into<String>,
        status: impl Into<String>,
        detail: Option<String>,
    ) {
        self.emit(ProgressEvent::StepFinished {
            step: step.into(),
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
        });
    }

    pub fn agent_token(&self, delta: impl Into<String>) {
        self.emit(ProgressEvent::AgentToken {
            delta: delta.into(),
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

/// A reporter that records every message, for tests that assert on the emitted
/// stream — this module's unit tests and other modules' (e.g. the live research
/// smoke in `pipeline`, which checks request-row group attribution). Test builds
/// only.
#[cfg(test)]
#[derive(Default)]
pub struct RecordingReporter(std::sync::Mutex<Vec<ProgressMessage>>);

#[cfg(test)]
impl RecordingReporter {
    /// Snapshot of every message reported so far.
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
        ctx.run_started("Weekly report");
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
