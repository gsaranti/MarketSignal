# Current session handoff

## What happened

The pending **`live-calibration-fixes` PR merged (#33)**, then the **first live run of the new config** (`MAIN=claude-opus`, `BULL/BEAR/BALANCED=gpt-5`) ran clean end-to-end and **validated three things at once**: the `string_or_seq` deserializer now holds on the **Anthropic _main_ path** (not just analysts — the run-killer's last unverified arm), all **three deferred UX fixes** (subtitle/title unification, title-in-toolbar, footer bar), and a strong qualitative read (lenses applied, low prose repetition, worked-examples not missed on opus). The user judged this config **better than last session's** gpt-5-main/sonnet-analysts → now the **leaning default**, tentative (one run, confounded — both axes swapped + a richer news week; [[live-config-opus-main-leaning]]). Then three app-shell PRs landed: **#34** official app icon (the design system's own `✻` sextile, oxblood-on-cream) + larger default window (1180×760, min 900×600); **#35** **dev/prod data-store separation** (debug builds nest under `dev/`, `MARKET_SIGNAL_DATA_DIR` override — `tauri dev` no longer touches production data); **#36** Codex-review fixes — **reverted the progress-bar sweep** (last session's "fix" had regressed a design-system-compliant static bar into a rejected loading-shimmer; now a steady 1px rule) + documented the storage location in `docs/storage.md`.

## Current state

`main` @ **`621be5d`**, all session work merged (#33–#36), tree clean, no work in flight, no code owed. A `dev/` store now exists in the app-data dir (the separation is live). Leaning live-run config: **`MAIN=claude-opus`, analysts=`gpt-5`** (supersedes the old gpt-5-main/sonnet lock, pending a few more runs to firm).

## Open questions

All **live-run / calibration** (no code owed):
- **Confirm the opus-main leaning** across more weeks, or do the clean controlled A/B (replay one run's captured packet swapping only the synthesizer — likely not worth building). This run flipped both model axes, so it's a new-config read, not the controlled Opus transfer-check ([[live-config-opus-main-leaning]]).
- **Cadence-const calibration (Run B)** — back-window caps + research-threshold clamps/anchor still need genuinely-spaced runs (multi-session); don't re-implement the curves ([[manual-pivot-cadence-windows]]).
- **Empirical skills calibration** — which of the 16 lenses get ignored across main + analysts; prose-repetition trended good on opus but wants more runs ([[skills-forcing-function-only]]).
- **Title coherence** (Codex #2) — left prompt-level (verified-good live); the only check worth adding is structural (app derives the body subtitle from `title`) if a model ever diverges. Worked-examples: leaned skip, opus confirmed not-missed — effectively resolved.

## Where to start

No code owed — pick by appetite. Either **another live run** to firm the opus-main default and begin cadence calibration (keys in `keys.env`; `tauri dev` now auto-isolates to `dev/`, so **no isolate/restore dance** — [[live-model-smoke]], [[gui-screenshot-audit]]), **or** a bundled **`tauri build`** to see the new icon in the real Dock and sanity-check the release build (unsigned → right-click-Open once).
