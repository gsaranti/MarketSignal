//! Diagnostic thought-log capture (`docs/run-tracking.md §Thought-log capture`):
//! a decorating [`ProgressReporter`] that appends every streamed *thinking*
//! delta to per-stream text files under one per-run folder, so a run's
//! reasoning survives the tracker's transient panes (the 2026-08-10 attempt-1
//! failure analysis rested on screenshots of them). Thoughts only — the main
//! agent's report body (`AgentToken`) persists as the report itself, and the
//! non-thinking events are run structure, already owned by the stderr tee.
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

use crate::progress::{ProgressEvent, ProgressMessage, ProgressReporter};

/// How many run folders survive pruning, newest first — bounded accumulation
/// across dev runs (~kilobytes each, so this is history depth, not a cost
/// question). Pruning runs only after a run's **first delta has landed on
/// disk**, keeping [`THOUGHT_LOG_RETENTION`] − 1 prior folders beside this
/// run's own; a run that captures nothing — or whose capture fails before the
/// first write — prunes nothing, so an old log is never deleted without a
/// replacement existing.
pub const THOUGHT_LOG_RETENTION: usize = 10;

/// A thinking-capture decorator around the live reporter. Constructed per run
/// by `live_run_context` when the gate is on; everywhere else (tests, noop
/// contexts) it simply never exists.
pub struct ThoughtLogSink {
    inner: Arc<dyn ProgressReporter>,
    /// This run's own folder. Created lazily on the first captured delta, so a
    /// run that streams no thinking (a quick check, an early cancel) leaves no
    /// empty folder behind.
    dir: PathBuf,
    /// Open appenders keyed by sanitized stream file name.
    files: Mutex<HashMap<String, File>>,
    /// Latched by the first capture error; the sink then forwards only.
    disabled: AtomicBool,
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
    /// first *successfully written* delta, so a run that streams no thinking
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

    /// The stream file a captured event appends to; `None` for everything the
    /// sink deliberately ignores.
    fn stream_file(event: &ProgressEvent) -> Option<(String, &str)> {
        match event {
            ProgressEvent::AgentThinking { delta } => Some(("main-agent.txt".into(), delta)),
            ProgressEvent::AnalystThinking { posture, delta } => {
                Some((format!("analyst-{}.txt", sanitize(posture)), delta))
            }
            ProgressEvent::StepThinking { step, delta } => {
                Some((format!("{}.txt", sanitize(step)), delta))
            }
            _ => None,
        }
    }

    fn append(&self, file_name: String, delta: &str) {
        if self.disabled.load(Ordering::Relaxed) || delta.is_empty() {
            return;
        }
        if let Err(e) = self.try_append(&file_name, delta) {
            // One line, once: capture is diagnostics and must cost the run
            // nothing, so the first error retires it for the rest of the run.
            self.disabled.store(true, Ordering::Relaxed);
            eprintln!(
                "[thought-log] capture disabled for this run ({}): {e}",
                self.dir.display()
            );
        }
    }

    fn try_append(&self, file_name: &str, delta: &str) -> std::io::Result<()> {
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
            files.insert(file_name.to_string(), file);
        }
        let file = files.get_mut(file_name).expect("inserted above");
        file.write_all(delta.as_bytes())?;
        if first_capture {
            // Prune only now, with the first delta already on disk: a capture
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
        if let Some((file_name, delta)) = Self::stream_file(&message.event) {
            self.append(file_name, delta);
        }
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
