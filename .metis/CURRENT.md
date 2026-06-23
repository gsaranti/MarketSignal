# Current session handoff

## What happened

The **first real live GUI run** (gpt-5 main / Sonnet analysts) of the "feature-complete" app **exposed a run-killing bug and several UX gaps**, so the session pivoted from calibration to fixing them. Root cause (found via an added diagnostic): the **analyst stage was silently discarding completed runs** because **Anthropic tool-use returns array fields as JSON-encoded strings** (`stop_reason: tool_use` — *not* truncation; the initial `MAX_TOKENS` hypothesis was wrong). Fixed with a **`string_or_seq` lenient deserializer** on both `ReviewEnvelope` and `ResponseEnvelope` (also protects a Claude main agent). Also fixed: **chart U+2011 unicode-hyphen** breaking chart JSON (`renderChart` normalization + ASCII-numerics prompt rule); **reports never emitting tables** (prompt now directs them — render validated live); **date-only displays** (added time to sidebar/toolbar); a new **per-issue `title`** field (agent contract → schema → prompt → summary → sidebar → docs); **static progress bar → indeterminate sweep**. An external **Codex review (3 findings) — all verified valid and fixed** (title coherence via toolbar + prompt-subtitle unification, bounded diagnostic, doc). Calibration read from the one good run: prompt-rigor working, lenses applied, low prose repetition, worked-examples low-value on gpt-5; the ERP "oddity" was real data.

## Current state

All work is on branch **`live-calibration-fixes`** — **4 commits, pushed to origin, NOT merged** (PR not opened). `origin/main` unchanged at `7c2011c`. Offline-verified throughout: clippy clean, **410 Rust tests**, build + **86 Vitest + 40 node**. The real app-data dir was isolated during the GUI runs and **restored**; no app processes left running. **Three changes are test-verified but NOT yet live-validated** (need a generated report): the prompt-subtitle/title unification, the title-in-toolbar visual (esp. a long headline), and the progress-bar sweep motion. Locked live-run config: `MAIN=gpt-5`, `BULL/BEAR/BALANCED=claude-sonnet`.

## Open questions

All **live-run only**:
- **Cadence-const calibration (Run B)** — back-window caps (`EARNINGS_BACK_MAX_DAYS=31` / `CALENDAR_BACK_MAX_DAYS=45`) via `captured_at` injection; research-threshold clamps/anchor fire-vs-no-fire needs genuinely-spaced runs (multi-session). Don't re-implement the curves ([[manual-pivot-cadence-windows]]).
- **Opus step-6 transfer-check** — now **unblocked** (the deserializer protects a Claude main agent): hold a baseline snapshot fixed, swap `MAIN→claude-opus`, compare the model-specific prose verdicts (worked-examples keep/revert, lens repetition).
- **Worked-examples** — lean *skip* on gpt-5; confirm against a run ([[skills-forcing-function-only]]).
- **Live-validate** the three deferred fixes above.

## Where to start

**Open + merge the PR** for `live-calibration-fixes` (4 commits). Then **one live run** (gpt-5/Sonnet; keys in `keys.env`, [[live-model-smoke]]) to live-validate the deferred fixes and begin cadence-const calibration. Isolate the real app-data dir before launching and restore after ([[gui-screenshot-audit]]); FMP ~25–30 calls/run (250/day fine), GDELT 429-prone but fail-soft. Drive the GUI via `osascript`/`screencapture` or write a headless full-E2E harness.
