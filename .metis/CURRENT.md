# Current session handoff

## What happened

Ran a **full-conformance audit of all 9 existing Vue components** against the
design system (appearance) and `frontend-craft` (robustness) — 9 parallel
auditors over a shared rubric — then fixed the gaps and shipped to `main`
(`f9ea738`). The core finding: the report-side chrome was already token-clean;
the real debt was **interaction/robustness states the design package never
specified**. Landed two **systemic package extensions** in
`colors_and_type.css` — a `.btn:disabled` treatment + `:not(:disabled)` hover
guards (fixes 5 disabled buttons that looked active) and a **global
prefers-reduced-motion guard** — plus the two High fixes (the warning-dismiss
oxblood-on-oxblood hover where the glyph vanished; the disabled-button gap) and
a spread of a11y fixes (always-mounted warning live region, terminal-outcome
`aria-live`, delete-confirm focus management, keyboard-scrollable report,
tabular numerals, `aria-current="page"`). **Resolved + documented the
surface-title casing rule:** static pane names = 13px uppercase tracked;
dynamic content/status headlines = sentence-case. Build + 131 tests green;
Metis review **approve-with-nits** (the one nit, a fill-token-as-text, now
documented in-code).

## Current state

Working tree clean except this file; `f9ea738` pushed to `main`. **Nothing in
flight.** The "report-side chrome" entry point into the component-restyle phase
is now **complete** — every existing component conforms. Two low-severity a11y
niceties were **consciously deferred** (sidebar report-list ARIA
`role=list/listitem` — DOM-restructure/layout risk; ConnectionTestRow
disabled-test reason reachability). Optional tidy-ups noted but not done:
tokenize the shared 50px toolbar seam as `--toolbar-seam`; a visual smoke via
`npm run tauri:demo`.

## Open questions

- **Component-restyle phase:** report-side/shared chrome is done; the remaining
  work is the **analytical register** (Portfolio + Trade Opportunities), which
  has **zero components built** and stays **M5-gated**. No non-gated UI
  conformance work remains.
- **Deferred a11y niceties** (sidebar list semantics; ConnectionTestRow
  disabled-reason) — pick up if wanted; both are frontend-craft niceties, not
  design-system violations.
- **Research-layer M5-calibration tier** — parked by intent (evidence-quality
  combining formula, claim-quorum thresholds, Tavily-as-calibrator). User
  preference: **no new Tavily**.
- **Portfolio holding-card overflow** — specified in the design page spec,
  needs handling at implementation (M5-gated).
- **Standing backlog** unchanged (implementation-time schemas, paid-FMP report
  enrichment, cross-job isolation, 35B residency benchmark, BUILD.md
  compression) — gated on M5 / paid-FMP.

## Where to start

The conformance pass is shipped and reviewed — no follow-up there. The next
substantive UI work (the analytical register) is **M5-gated**, so when ready,
the live options are: re-raise the parked research-layer M5-calibration tier,
continue the **job-doc deepening initiative**, or knock out the two deferred
a11y niceties / the `--toolbar-seam` tidy-up. The local-suite Portfolio/TO UI
itself stays M5-gated.
