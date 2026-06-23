# Current session handoff

## What happened

Two things, no code changed (tree clean at **`621be5d`**). First, **diagnosed why the GDELT news request "fails on every run"**: it's an **escalating per-IP 429 lockout**, not a code bug or malformed query. Verified live from the user's own dev-machine egress IP — our exact request on a *cold* IP returns 250 valid articles (implementation confirmed correct against the DOC 2.0 docs; the app makes exactly one request/run), but a cold IP gets **one success then locks out**: 8s and 16s spacing still 429 (well past the "5s" claim), no `Retry-After`, ~9–10s tarpit. **A retry is contraindicated** (tested live — doesn't recover within any sane backoff, risks extending the lockout); the single-shot fail-soft is correct. Most likely cause: self-inflicted clustering from repeated dev runs. Findings folded into the [[gdelt-doc-api-rate-limit]] memory. Second, **scoped and LOCKED the next build**: enable model **thinking** + **stream thoughts to the job tracker** for the main agent AND all three analysts, across every capable model — full detail persisted in the [[thinking-streaming-plan]] memory.

## Current state

The thinking/thought-streaming plan is **locked but unbuilt** — no code owed elsewhere. Decision made this session: **FULL scope, incl. the OpenAI Responses-API migration** (user chose this over Anthropic-only / move-analysts-to-Claude). Key shape (full spec + code refs in the [[thinking-streaming-plan]] memory):
- **Capability split.** Anthropic (`claude-opus`/`-sonnet`/`-haiku`): thinking is OFF now; turning it on streams `thinking_delta`, **but is blocked by the forced-tool structured output → must swap to `output_config.format`** (thinking-compatible). OpenAI (`gpt-5`/`-mini`): reason internally but Chat Completions can't expose it → **migrate the OpenAI arm to the Responses API** (reasoning summaries).
- **Analysts** are currently fully non-streaming → convert to streaming, **thoughts-only** (not their review body); needs **per-posture routing** (bull/bear/balanced) in the tracker, which today has only a single main-agent `agent_token` channel.
- **Phases:** 0 verify-against-live-docs → 1 main agent Anthropic (swap + thinking + thought channel + tracker UI) → 2 analysts Anthropic → 3 OpenAI Responses-API (largest chunk) → 4 capability gate `thinking_config(model)->Option` + docs + full verify.

## Open questions

- **Calibration baseline reset (new, important):** turning thinking on changes behavior the user judged *without* it — so the opus-main leaning + the empirical-skills / prose-repetition reads all want a **fresh live run after Phase 1–2** ([[skills-forcing-function-only]], [[live-config-opus-main-leaning]]).
- **GDELT production health:** confirm with one isolated **cold-IP** run (≥10 min of zero GDELT traffic first); the per-IP lockout is environmental, not code-fixable.
- Carried-forward live-run/calibration (no code owed): confirm the **opus-main leaning** across more weeks; **cadence-const Run B** (back-window caps + research-threshold clamps/anchor, [[manual-pivot-cadence-windows]]).

## Where to start

Pick up the locked **thinking/thought-streaming build** at **Phase 0** (verify the API specifics live — Anthropic `output_config.format`+thinking & `thinking_delta` shape; haiku-4.5's `budget_tokens`; OpenAI Responses-API reasoning streaming), then **Phase 1** — the **forced-tool → `output_config.format` swap is the unblocker, do it first**. Full spec in the [[thinking-streaming-plan]] memory. Keys in `keys.env`; `tauri dev` auto-isolates to `dev/`.
