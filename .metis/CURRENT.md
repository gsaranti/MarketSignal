# Current session handoff

## What happened

A **docs-only design session** (M5-gated, unbuilt) settling the local-suite
web-research **provisioning + gating + connection-status UI** — the follow-on to
the prior fetch-layer thread. Committed `4c623af` to `main` (6 docs). Load-bearing
calls:

- **Provisioning.** SearXNG is self-hosted via an **app-shipped pinned
  `docker-compose.yml` + `settings.yml`** (config baked in → *resolves the prior
  "how does the SearXNG config get set up" ambiguity*); OrbStack pointer. Ollama is
  user-installed with **guided in-app install/pull**. App bundles **neither** —
  app-bundling SearXNG and embedding the inference engine were both **rejected as
  dominated**.
- **★ Presence-vs-connectivity gating (the load-bearing decision).** Presence of
  config values locks the Run buttons + raises a persistent **"local models not
  configured"** warning; **connectivity** is checked only at the run-gate + manual
  Test Connection — **no startup probe, no poll** (config-set-but-down is blind on
  re-open, accepted, cloud-report-consistent). This **replaced an earlier
  startup-probe + auto-clear model** explored mid-session — don't retry it.
- **Connection-status UI** (Ollama gate-bearing / SearXNG degradation-only, never a
  warning category) + a **pre-run SearXNG consent modal** on can't-serve-search
  (incl. 403), job-specific/Tavily-conditional copy, TO-without-Tavily flagged
  *not recommended*. **Brave = contingency, not primary.**

Survived **3 Codex review rounds (9 findings, all fixed)**. Memory
`web-research-fetch-layer-decisions.md` + `MEMORY.md` index synced.

## Current state

Working tree clean except this file; `4c623af` pushed to `main` (docs-only — no
build/test gate touched). **Nothing in flight.** The provisioning/gating/UI thread
is fully captured — 6 docs hold the as-built design, the
`web-research-fetch-layer-decisions.md` memory holds the chat-only rationale +
rejected paths. All M5-gated/unbuilt.

## Open questions

- **BUILD.md** still omits the rendered-retrieval tier **and** this session's
  provisioning / presence-vs-connectivity gating / connection-status decisions —
  likely wait (M5-gated/unbuilt, per the as-built doctrine), but the gap has grown.
- **WKWebView render-tier spike** — the open build work for the rendered-retrieval
  tier: cookie-store sharing (Connected-Sources login → hidden fetch webview) + DOM
  read-back to Rust over IPC. M5-gated.
- **Scrapling / external stealth browser** — spike-gated fallback only, evaluated on
  M5 data *if* a must-have paid source defeats the authenticated WKWebView.
- **Component-restyle:** the **analytical register** (Portfolio + Trade
  Opportunities) is M5-gated, zero components built — now includes the
  connection-status rows + pre-run modal designed this session.
- **Deferred a11y niceties** (sidebar list semantics; ConnectionTestRow
  disabled-reason) + the `--toolbar-seam` tidy-up — pick up if wanted.
- **Research-layer M5-calibration tier** — parked by intent; **no new Tavily**.
- **Standing backlog** unchanged (implementation-time schemas, paid-FMP report
  enrichment, cross-job isolation, 35B residency benchmark, Portfolio holding-card
  overflow, BUILD.md compression) — gated on M5 / paid-FMP.

## Where to start

The web-research provisioning/gating/UI thread is closed and captured — no
follow-up needed unless resuming it. Live non-gated options: the **job-doc
deepening initiative** or the **deferred a11y niceties / `--toolbar-seam`**. If
resuming web-research, the first build action is the **WKWebView render-tier spike**
(cookie sharing + DOM read-back over IPC). All substantive local-suite UI/build work
stays **M5-gated**.
