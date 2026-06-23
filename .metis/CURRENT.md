# Current session handoff

## What happened

Ran the deferred **batched live-verification + calibration pass on `main` — all green** — then **built + installed the first real release (v0.1.0)** for daily use. Verification: two cheap adapter smokes + one full GUI run (MAIN=`claude-opus`, analysts=`gpt-5`). Every deferred wire flag resolved — Anthropic `output_config.format` needs **no `name`**, haiku-4-5 streams non-empty thinking, OpenAI Responses `text.format`/`store:false`/`max_output_tokens` all accepted; SSE→markdown exact for both providers. **OpenAI org IS verification-approved** — gpt-5 analyst reasoning panes populated live (not empty); opus main reasoning pane populated. Calibration read **strongly positive** even cold-start (opus+thinking: explicit conviction calibration, multi-condition falsifiable triggers, clean lens integration). GDELT 429→fail-soft (expected). Then `npm run tauri build` → `/Applications/Market Signal.app` (ad-hoc signed, quarantine cleared); fresh-started the production store (21 old test reports backed up to `…BACKUP-2026-06-23`) and **pre-seeded `app_settings`** (opus-main/gpt-5-analysts + 5 keys) → gate green on first launch, verified.

## Current state

**App is built, installed, configured, and live for daily use** (running; no code changed this session, tree clean, nothing owed). Operational fact for any future build: the **installed app reads config+keys from SQLite `app_settings`, NOT env** — env only drives `tauri dev` ([[release-build-install]]). Rebuilding+reinstalling preserves config/reports/memory (data dir keyed by bundle id) — no re-seed on upgrade. The user's **first real report is not yet run** — left to them to fire (it becomes day-1 of live thesis continuity); the full generate path is already proven by the verification run.

## Open questions

- **Cadence Run B** stays open — the baseline delta-engine + vector-memory recall are still live-unexercised (cold-start had neither); they self-exercise on the user's **2nd real report**. Deferred ([[manual-pivot-cadence-windows]]).
- **Per-posture analyst panes** (Bull/Bear/Balanced headers) unconfirmed — the fast analyst phase was caught mid-scroll; eyeball on a future run. Reasoning streaming itself is confirmed.
- **opus-main leaning** now has two strong runs (firming, not a clean A/B) — keep accumulating ([[live-config-opus-main-leaning]]). Optional carry: the **worked-examples prompt enhancement**.

## Where to start

No code owed — the app is in real use, so next session likely **reacts to live usage**. When the user fires their **2nd real report**, the cadence delta + memory-recall paths finally exercise live (Run B closes itself) — watch those + eyeball the per-posture reasoning panes. Otherwise small polish: the worked-examples prompt. Build/install method (Gatekeeper + re-seed caveat) is in [[release-build-install]].
