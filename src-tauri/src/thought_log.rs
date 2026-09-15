//! Diagnostic thought-log capture (`docs/run-tracking.md §Thought-log capture`):
//! a decorating [`ProgressReporter`] that appends every streamed *thinking*
//! delta to per-stream text files under one per-run folder, so a run's
//! reasoning survives the tracker's transient panes (the 2026-08-10 attempt-1
//! failure analysis rested on screenshots of them). Thoughts only — the main
//! agent's report body (`AgentToken`) persists as the report itself, and the
//! non-thinking events are run structure, already owned by the stderr tee.
//!
//! The one structural addition is the **call fence**: every local-model call's
//! boundary events (`ModelCallStarted` / `ModelCallFinished`) write a header
//! and a trailer line into the owning step's file, so a holding's file reads
//! as a call timeline — the stage, model, controls and start time above each
//! call's thinking, its outcome, elapsed time and token counts below — instead
//! of every call's reasoning run together. Fences carry labels and counts
//! only, never prompt or body text.
//!
//! Best-effort by contract: the sink may never fail or reorder the run it
//! observes. Every message is forwarded to the inner reporter first; any
//! capture I/O error disables capture for the rest of the run with one stderr
//! line. Appends are **synchronous and unbuffered by design** — a mid-run
//! failure leaves everything streamed so far on disk, and the crash case is
//! exactly the one this exists for. The cost side of that choice is stated
//! honestly: deltas are small coalesced chunks at local-model token rates, so
//! healthy-disk appends are negligible, but a stalled disk would stall the
//! stream loop — accepted for a debug-gated diagnostic whose value is
//! crash-honesty (an async writer would lose the in-flight tail at exactly
//! the crash that matters).

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::progress::{elapsed_label, ProgressEvent, ProgressMessage, ProgressReporter};

/// How many run folders survive pruning, newest first — bounded accumulation
/// across dev runs (~kilobytes each, so this is history depth, not a cost
/// question). Pruning runs only after a run's **first capture has landed on
/// disk** (a call fence or a thinking delta), keeping [`THOUGHT_LOG_RETENTION`]
/// − 1 prior folders beside this run's own; a run that captures nothing — or
/// whose capture fails before the first write — prunes nothing, so an old log
/// is never deleted without a replacement existing.
pub const THOUGHT_LOG_RETENTION: usize = 10;

/// A thinking-capture decorator around the live reporter. Constructed per run
/// by `live_run_context` when the gate is on; everywhere else (tests, noop
/// contexts) it simply never exists.
pub struct ThoughtLogSink {
    inner: Arc<dyn ProgressReporter>,
    /// This run's own folder. Created lazily on the first capture (a call
    /// fence or a thinking delta), so a run that issues no local call and
    /// streams no thinking (a quick check, an early cancel) leaves no empty
    /// folder behind.
    dir: PathBuf,
    /// Open appenders keyed by sanitized stream file name.
    files: Mutex<HashMap<String, Appender>>,
    /// Latched by the first capture error; the sink then forwards only.
    disabled: AtomicBool,
}

/// One stream file plus whether its last byte was a newline — the fence
/// guard: a header or trailer always starts on its own line, even after a
/// thinking delta that ended mid-sentence, while deltas append verbatim.
struct Appender {
    file: File,
    at_line_start: bool,
}

/// What one captured event appends: a thinking delta verbatim, or a fence
/// line that must begin at a line start.
enum Capture {
    Delta(String),
    Line(String),
}

impl ThoughtLogSink {
    /// Wrap `inner`, deriving the run folder name
    /// `<UTC yyyymmdd-hhmmss>-<8 alphanumerics of run_id>` — a wall-clock name,
    /// deliberately: this is a diagnostic file directory a human browses, not a
    /// store an identity-or-lifecycle selection reads, so the insertion-order
    /// rule does not bind and lexical order doubling as chronological order is
    /// the whole point. The id half is normalized to exactly eight
    /// alphanumerics so every folder this sink writes matches
    /// [`looks_like_run_dir`]. Nothing is pruned here — pruning waits for the
    /// first *successfully written* capture, so a run that streams no thinking
    /// (a quick check, a blocked or skipped attempt) or whose capture fails
    /// outright can never delete an old log without leaving a replacement.
    pub fn attach(inner: Arc<dyn ProgressReporter>, base: &Path, run_id: &str) -> Self {
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let id8: String = run_id.chars().filter(char::is_ascii_alphanumeric).take(8).collect();
        let id8 = format!("{id8:0<8}");
        Self {
            inner,
            dir: base.join(format!("{stamp}-{id8}")),
            files: Mutex::new(HashMap::new()),
            disabled: AtomicBool::new(false),
        }
    }

    /// The stream file a captured event appends to and what it appends;
    /// `None` for everything the sink deliberately ignores. A call boundary
    /// lands in its stamped step's file — the same file that step's thinking
    /// deltas use — or in `run.txt` when it fired with no step open.
    fn stream_file(event: &ProgressEvent) -> Option<(String, Capture)> {
        match event {
            ProgressEvent::AgentThinking { delta } => {
                Some(("main-agent.txt".into(), Capture::Delta(delta.clone())))
            }
            ProgressEvent::AnalystThinking { posture, delta } => Some((
                format!("analyst-{}.txt", sanitize(posture)),
                Capture::Delta(delta.clone()),
            )),
            ProgressEvent::StepThinking { step, delta } => {
                Some((format!("{}.txt", sanitize(step)), Capture::Delta(delta.clone())))
            }
            ProgressEvent::ModelCallStarted { step, .. }
            | ProgressEvent::ModelCallFinished { step, .. } => {
                let file = match step {
                    Some(step) => format!("{}.txt", sanitize(step)),
                    None => "run.txt".to_string(),
                };
                Some((file, Capture::Line(render_fence(event))))
            }
            _ => None,
        }
    }

    fn append(&self, file_name: String, capture: Capture) {
        let empty = match &capture {
            Capture::Delta(text) | Capture::Line(text) => text.is_empty(),
        };
        if self.disabled.load(Ordering::Relaxed) || empty {
            return;
        }
        if let Err(e) = self.try_append(&file_name, &capture) {
            // One line, once: capture is diagnostics and must cost the run
            // nothing, so the first error retires it for the rest of the run.
            self.disabled.store(true, Ordering::Relaxed);
            eprintln!(
                "[thought-log] capture disabled for this run ({}): {e}",
                self.dir.display()
            );
        }
    }

    fn try_append(&self, file_name: &str, capture: &Capture) -> std::io::Result<()> {
        let mut files = match self.files.lock() {
            Ok(g) => g,
            // A poisoned lock means a prior capture panicked; treat as an
            // error rather than propagating the panic into the run.
            Err(_) => return Err(std::io::Error::other("thought-log lock poisoned")),
        };
        let first_capture = files.is_empty();
        if !files.contains_key(file_name) {
            fs::create_dir_all(&self.dir)?;
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join(file_name))?;
            files.insert(
                file_name.to_string(),
                Appender {
                    file,
                    at_line_start: true,
                },
            );
        }
        let appender = files.get_mut(file_name).expect("inserted above");
        let text = match capture {
            Capture::Delta(text) => text.as_str(),
            Capture::Line(text) => text.as_str(),
        };
        if matches!(capture, Capture::Line(_)) && !appender.at_line_start {
            appender.file.write_all(b"\n")?;
        }
        appender.file.write_all(text.as_bytes())?;
        appender.at_line_start = text.ends_with('\n');
        if first_capture {
            // Prune only now, with the first capture (a fence header or a
            // delta) already on disk: a capture
            // that failed anywhere above latched the disable without pruning,
            // so a failed run can never delete an old log while producing no
            // replacement. This run's own folder is exempt by name, so even a
            // backwards clock step (which would make it sort oldest) cannot
            // turn it into its own prune victim.
            if let (Some(base), Some(own)) = (self.dir.parent(), self.dir.file_name()) {
                prune_run_dirs(base, THOUGHT_LOG_RETENTION.saturating_sub(1), Some(own));
            }
        }
        Ok(())
    }
}

impl ProgressReporter for ThoughtLogSink {
    fn report(&self, message: &ProgressMessage) {
        // Forward first: the live tracker must never wait on disk.
        self.inner.report(message);
        if let Some((file_name, capture)) = Self::stream_file(&message.event) {
            self.append(file_name, capture);
        }
    }
}

/// The fence lines a call's boundary events write. The header opens the call
/// with its number, stage, model, controls, prompt size and UTC start time;
/// the trailer closes it with the outcome, elapsed time and the daemon's counts
/// (or a failure's capped detail), then a blank line so the next call stands
/// apart. `====` is the fixed prefix a reader splits on.
fn render_fence(event: &ProgressEvent) -> String {
    match event {
        ProgressEvent::ModelCallStarted {
            call,
            stage,
            model,
            think,
            streamed,
            tools,
            format,
            num_ctx,
            num_predict,
            prompt_chars,
            ..
        } => {
            let mut parts = vec![
                format!("==== call {call}"),
                stage.clone(),
                model.clone(),
                match think {
                    Some(true) => "think on",
                    Some(false) => "think off",
                    None => "think default",
                }
                .to_string(),
                if *streamed { "streamed" } else { "non-streaming" }.to_string(),
            ];
            if *tools {
                parts.push("tools".into());
            }
            if *format {
                parts.push("format".into());
            }
            if let Some(n) = num_ctx {
                parts.push(format!("ctx {n}"));
            }
            if let Some(n) = num_predict {
                parts.push(format!("predict {n}"));
            }
            parts.push(format!("prompt {prompt_chars} chars"));
            parts.push(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
            format!("{}\n", parts.join(" | "))
        }
        ProgressEvent::ModelCallFinished {
            call,
            status,
            detail,
            elapsed_ms,
            prompt_tokens,
            generated_tokens,
            done_reason,
            ..
        } => {
            let mut parts = vec![
                format!("==== end {call}"),
                status.clone(),
                elapsed_label(*elapsed_ms),
            ];
            if let Some(n) = prompt_tokens {
                parts.push(format!("prompt {n} tok"));
            }
            if let Some(n) = generated_tokens {
                parts.push(format!("generated {n} tok"));
            }
            if let Some(reason) = done_reason {
                parts.push(reason.clone());
            }
            if let Some(detail) = detail {
                // One line: a detail's own newlines would break the fence.
                parts.push(detail.replace(['\n', '\r'], " "));
            }
            format!("{}\n\n", parts.join(" | "))
        }
        _ => String::new(),
    }
}

/// Replace every character outside `[A-Za-z0-9._-]` so a stream key can never
/// escape its folder or fail to name a file — `holding-BRK/B` (slash-notation
/// symbols are real Schwab identities) writes `holding-BRK_B.txt`.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "stream".to_string()
    } else {
        cleaned
    }
}

/// Delete this sink's oldest run folders beyond `keep`, newest-by-name first,
/// never touching `exempt` (the calling run's own folder). Only entries
/// matching the sink's own exact `<yyyymmdd-hhmmss>-<8 alnum>` shape are
/// counted or deleted — anything else in the directory (a user's notes, a
/// stray file, a `20260810-120000-attempt-2-analysis` lookalike with the
/// wrong suffix) is not this sink's to remove. Best-effort: a failed removal
/// is skipped.
pub fn prune_run_dirs(base: &Path, keep: usize, exempt: Option<&std::ffi::OsStr>) {
    let Ok(entries) = fs::read_dir(base) else {
        return; // No directory yet — nothing to prune.
    };
    let mut runs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| Some(e.file_name().as_os_str()) != exempt)
        .filter(|e| e.path().is_dir())
        .filter(|e| looks_like_run_dir(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    // Folder names are `<yyyymmdd-hhmmss>-<id8>`, so descending lexical order
    // is newest-first.
    runs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for old in runs.iter().skip(keep) {
        let _ = fs::remove_dir_all(old);
    }
}

/// Exactly `<8 digits>-<6 digits>-<8 alphanumerics>` — the shape
/// [`ThoughtLogSink::attach`] writes, and nothing looser: a length or suffix
/// mismatch (a user's dated analysis folder) must never read as prunable.
fn looks_like_run_dir(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 24
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'-'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'-'
        && bytes[16..24].iter().all(u8::is_ascii_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Counts forwarded messages, proving pass-through is unconditional.
    struct CountingReporter(AtomicUsize);
    impl ProgressReporter for CountingReporter {
        fn report(&self, _message: &ProgressMessage) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn msg(event: ProgressEvent) -> ProgressMessage {
        ProgressMessage { run_id: "r1".into(), seq: 0, event }
    }

    fn sink_under(dir: &Path) -> (Arc<CountingReporter>, ThoughtLogSink) {
        let inner = Arc::new(CountingReporter(AtomicUsize::new(0)));
        let sink = ThoughtLogSink::attach(inner.clone(), dir, "abcd1234-run");
        (inner, sink)
    }

    #[test]
    fn thinking_streams_land_in_per_stream_files_and_everything_forwards() {
        let tmp = tempfile::tempdir().unwrap();
        let (inner, sink) = sink_under(tmp.path());

        sink.report(&msg(ProgressEvent::AgentThinking { delta: "alpha ".into() }));
        sink.report(&msg(ProgressEvent::AgentThinking { delta: "beta".into() }));
        sink.report(&msg(ProgressEvent::AnalystThinking {
            posture: "bull".into(),
            delta: "horns".into(),
        }));
        sink.report(&msg(ProgressEvent::StepThinking {
            step: "holding-AAPL".into(),
            delta: "cored".into(),
        }));
        // Slash-notation symbol: the file name sanitizes, never nests a dir.
        sink.report(&msg(ProgressEvent::StepThinking {
            step: "holding-BRK/B".into(),
            delta: "berkshire".into(),
        }));
        // Never captured: the report body and run structure.
        sink.report(&msg(ProgressEvent::AgentToken { delta: "report text".into() }));
        sink.report(&msg(ProgressEvent::RunStarted { label: "Run".into() }));

        assert_eq!(inner.0.load(Ordering::Relaxed), 7, "every message forwards");
        assert_eq!(fs::read_to_string(sink.dir.join("main-agent.txt")).unwrap(), "alpha beta");
        assert_eq!(fs::read_to_string(sink.dir.join("analyst-bull.txt")).unwrap(), "horns");
        assert_eq!(fs::read_to_string(sink.dir.join("holding-AAPL.txt")).unwrap(), "cored");
        assert_eq!(fs::read_to_string(sink.dir.join("holding-BRK_B.txt")).unwrap(), "berkshire");
        let names: Vec<String> = fs::read_dir(&sink.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 4, "no file for uncaptured events: {names:?}");
    }

    fn started(call: u64, stage: &str, step: Option<&str>) -> ProgressEvent {
        ProgressEvent::ModelCallStarted {
            call,
            stage: stage.into(),
            model: "qwen".into(),
            think: Some(true),
            streamed: true,
            tools: false,
            format: true,
            num_ctx: Some(131_072),
            num_predict: Some(65_536),
            prompt_chars: 4_200,
            step: step.map(str::to_string),
        }
    }

    fn finished_ok(call: u64, step: Option<&str>) -> ProgressEvent {
        ProgressEvent::ModelCallFinished {
            call,
            stage: "interpret AAPL".into(),
            status: "ok".into(),
            detail: None,
            elapsed_ms: 461_000,
            prompt_tokens: Some(41_203),
            generated_tokens: Some(8_921),
            done_reason: Some("stop".into()),
            step: step.map(str::to_string),
        }
    }

    #[test]
    fn call_fences_bracket_a_step_file_around_its_thinking() {
        let tmp = tempfile::tempdir().unwrap();
        let (inner, sink) = sink_under(tmp.path());
        let step = Some("holding-AAPL");
        sink.report(&msg(started(1, "interpret AAPL", step)));
        sink.report(&msg(ProgressEvent::StepThinking {
            step: "holding-AAPL".into(),
            delta: "weighing the trim".into(),
        }));
        // The delta ended mid-line: the trailer must still start its own line.
        sink.report(&msg(finished_ok(1, step)));
        assert_eq!(inner.0.load(Ordering::Relaxed), 3, "every message forwards");

        let text = fs::read_to_string(sink.dir.join("holding-AAPL.txt")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "header, thinking, trailer, blank: {text:?}");
        assert!(
            lines[0].starts_with(
                "==== call 1 | interpret AAPL | qwen | think on | streamed | format | \
                 ctx 131072 | predict 65536 | prompt 4200 chars | 20"
            ),
            "{}",
            lines[0]
        );
        assert!(lines[0].ends_with('Z'), "UTC start time closes the header: {}", lines[0]);
        assert_eq!(lines[1], "weighing the trim");
        assert_eq!(
            lines[2],
            "==== end 1 | ok | 7m41s | prompt 41203 tok | generated 8921 tok | stop"
        );
        assert_eq!(lines[3], "", "a blank line separates calls");
        assert!(text.ends_with("\n\n"));
    }

    #[test]
    fn a_non_thinking_call_leaves_adjacent_fences_in_the_same_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, sink) = sink_under(tmp.path());
        let step = Some("holding-AAPL");
        let mut distill = started(2, "distill AAPL", step);
        if let ProgressEvent::ModelCallStarted { think, streamed, tools, format, .. } = &mut distill {
            *think = Some(false);
            *streamed = false;
            *tools = true;
            *format = false;
        }
        sink.report(&msg(distill));
        sink.report(&msg(ProgressEvent::ModelCallFinished {
            call: 2,
            stage: "distill AAPL".into(),
            status: "ok".into(),
            detail: None,
            elapsed_ms: 12_000,
            prompt_tokens: None,
            generated_tokens: None,
            done_reason: None,
            step: step.map(str::to_string),
        }));
        let text = fs::read_to_string(sink.dir.join("holding-AAPL.txt")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[0].starts_with("==== call 2 | distill AAPL | qwen | think off | non-streaming | tools | ctx"),
            "{}",
            lines[0]
        );
        assert_eq!(lines[1], "==== end 2 | ok | 12s", "absent counts are omitted, not printed");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn a_failed_call_trailer_carries_its_detail_on_one_line() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, sink) = sink_under(tmp.path());
        sink.report(&msg(ProgressEvent::ModelCallFinished {
            call: 3,
            stage: "action AAPL".into(),
            status: "failed".into(),
            detail: Some("local model returned 500:\nrunner crashed".into()),
            elapsed_ms: 900,
            prompt_tokens: None,
            generated_tokens: None,
            done_reason: None,
            step: Some("holding-AAPL".into()),
        }));
        let text = fs::read_to_string(sink.dir.join("holding-AAPL.txt")).unwrap();
        assert_eq!(
            text,
            "==== end 3 | failed | 900ms | local model returned 500: runner crashed\n\n"
        );
    }

    #[test]
    fn an_unstamped_call_fences_into_run_txt() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, sink) = sink_under(tmp.path());
        sink.report(&msg(started(1, "probe", None)));
        sink.report(&msg(finished_ok(1, None)));
        assert!(sink.dir.join("run.txt").exists());
        let names: Vec<String> = fs::read_dir(&sink.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["run.txt".to_string()]);
    }

    #[test]
    fn a_run_with_no_thinking_leaves_no_folder_and_prunes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        // Ten prior runs at the retention cap: a thought-less run (a quick
        // check, a blocked attempt) must not spend one of them.
        for i in 0..10 {
            fs::create_dir(tmp.path().join(format!("20260801-1200{i:02}-abcd{i:04}"))).unwrap();
        }
        let (_, sink) = sink_under(tmp.path());
        sink.report(&msg(ProgressEvent::RunStarted { label: "Run".into() }));
        sink.report(&msg(ProgressEvent::AgentToken { delta: "text".into() }));
        assert!(!sink.dir.exists(), "lazy folder must not exist: {}", sink.dir.display());
        assert_eq!(
            fs::read_dir(tmp.path()).unwrap().count(),
            10,
            "all ten prior run folders survive a run that captured nothing"
        );
    }

    #[test]
    fn the_first_captured_delta_prunes_prior_runs_to_make_this_one_the_tenth() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..10 {
            fs::create_dir(tmp.path().join(format!("20260801-1200{i:02}-abcd{i:04}"))).unwrap();
        }
        let (_, sink) = sink_under(tmp.path());
        sink.report(&msg(ProgressEvent::AgentThinking { delta: "captured".into() }));
        assert!(sink.dir.exists(), "own folder created on first capture");
        assert_eq!(
            fs::read_dir(tmp.path()).unwrap().count(),
            10,
            "nine prior folders plus this run's own"
        );
        assert!(
            !tmp.path().join("20260801-120000-abcd0000").exists(),
            "the oldest prior folder was the one pruned"
        );
    }

    #[test]
    fn a_capture_error_disables_the_sink_but_never_the_forwarding() {
        let tmp = tempfile::tempdir().unwrap();
        // Make the run folder's parent a FILE so create_dir_all must fail.
        let blocked = tmp.path().join("blocked");
        fs::write(&blocked, b"a file where a directory must go").unwrap();
        let inner = Arc::new(CountingReporter(AtomicUsize::new(0)));
        let sink = ThoughtLogSink::attach(inner.clone(), &blocked, "abcd1234-run");

        sink.report(&msg(ProgressEvent::AgentThinking { delta: "lost".into() }));
        assert!(sink.disabled.load(Ordering::Relaxed), "first error latches");
        sink.report(&msg(ProgressEvent::AgentThinking { delta: "also lost".into() }));
        assert_eq!(inner.0.load(Ordering::Relaxed), 2, "forwarding survives capture failure");
    }

    #[test]
    fn pruning_keeps_the_newest_runs_and_never_touches_foreign_entries() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..12 {
            fs::create_dir(tmp.path().join(format!("20260801-1200{i:02}-abcd{i:04}"))).unwrap();
        }
        fs::create_dir(tmp.path().join("analysis-notes")).unwrap();
        // A dated user folder that MIMICS the prefix but not the exact shape —
        // sorts before every run folder, so a loose guard would delete it.
        fs::create_dir(tmp.path().join("20260101-000000-attempt-2-analysis")).unwrap();
        fs::write(tmp.path().join("loose.txt"), b"not a run dir").unwrap();

        prune_run_dirs(tmp.path(), 10, None);

        let mut names: Vec<String> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert!(names.contains(&"analysis-notes".to_string()), "foreign dir untouched");
        assert!(names.contains(&"loose.txt".to_string()), "foreign file untouched");
        assert!(
            names.contains(&"20260101-000000-attempt-2-analysis".to_string()),
            "a dated lookalike with the wrong suffix is not the sink's to remove"
        );
        let runs: Vec<&String> = names.iter().filter(|n| looks_like_run_dir(n)).collect();
        assert_eq!(runs.len(), 10, "kept the newest ten: {runs:?}");
        assert!(!names.contains(&"20260801-120000-abcd0000".to_string()), "oldest pruned");
        assert!(!names.contains(&"20260801-120001-abcd0001".to_string()), "second-oldest pruned");
        assert!(names.contains(&"20260801-120011-abcd0011".to_string()), "newest kept");
    }

    #[test]
    fn run_dir_shape_is_exact_and_attach_always_produces_it() {
        assert!(looks_like_run_dir("20260812-231500-9f3ab12c"));
        assert!(!looks_like_run_dir("20260810-120000-attempt-2-analysis"));
        assert!(!looks_like_run_dir("20260810-120000-abc"), "short suffix");
        assert!(!looks_like_run_dir("20260810-120000-"), "no suffix");
        assert!(!looks_like_run_dir("notes"));
        // attach normalizes any run id to the exact 8-alphanumeric suffix.
        let tmp = tempfile::tempdir().unwrap();
        let inner = Arc::new(CountingReporter(AtomicUsize::new(0)));
        for run_id in ["9f3ab12c-4e5d-6789", "x", ""] {
            let sink = ThoughtLogSink::attach(inner.clone(), tmp.path(), run_id);
            let name = sink.dir.file_name().unwrap().to_string_lossy().into_owned();
            assert!(looks_like_run_dir(&name), "attach must match the guard: {name}");
        }
    }


    #[test]
    fn a_failed_first_capture_prunes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..10 {
            fs::create_dir(tmp.path().join(format!("20260801-1200{i:02}-abcd{i:04}"))).unwrap();
        }
        let (inner, sink) = sink_under(tmp.path());
        // Occupy the stream file's path with a DIRECTORY so the open fails
        // after `create_dir_all` succeeded — the capture dies mid-setup.
        fs::create_dir_all(sink.dir.join("main-agent.txt")).unwrap();
        sink.report(&msg(ProgressEvent::AgentThinking { delta: "lost".into() }));
        assert!(sink.disabled.load(Ordering::Relaxed));
        assert_eq!(inner.0.load(Ordering::Relaxed), 1, "forwarding unaffected");
        for i in 0..10 {
            assert!(
                tmp.path().join(format!("20260801-1200{i:02}-abcd{i:04}")).exists(),
                "a failed capture must not have spent prior log {i}"
            );
        }
    }

    #[test]
    fn a_backdated_own_folder_is_never_its_own_prune_victim() {
        let tmp = tempfile::tempdir().unwrap();
        // Ten priors named in the FUTURE: a backwards clock step makes this
        // run's folder sort oldest, where an unexempted prune would eat it.
        for i in 0..10 {
            fs::create_dir(tmp.path().join(format!("20991231-1200{i:02}-abcd{i:04}"))).unwrap();
        }
        let (_, sink) = sink_under(tmp.path());
        sink.report(&msg(ProgressEvent::AgentThinking { delta: "kept".into() }));
        assert!(sink.dir.exists(), "own folder survives by exemption");
        assert_eq!(
            fs::read_to_string(sink.dir.join("main-agent.txt")).unwrap(),
            "kept",
            "the captured delta is intact"
        );
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 10, "nine futures plus own");
    }

    #[test]
    fn pruning_a_missing_base_is_a_quiet_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        prune_run_dirs(&tmp.path().join("never-created"), 10, None);
    }

    #[test]
    fn sanitize_replaces_path_hostile_characters_only() {
        assert_eq!(sanitize("holding-BRK/B"), "holding-BRK_B");
        assert_eq!(sanitize("holding-AAPL"), "holding-AAPL");
        assert_eq!(sanitize("a b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize(""), "stream");
    }
}
