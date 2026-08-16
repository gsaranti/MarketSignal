# Current session handoff

## What happened

Continued the clarity walk through
`logic-flow-docs/portfolio-analysis-logic-flow.md`, reaching Step 6. Mid-walk it
turned into a **design change**: Portfolio's **research reuse** was reworked from
a binary skip / graduated-depth model to **always-run seed-and-merge, never a
skip** — the research loop and distillation run **full every run for every
analyzed holding**, and cached research is purely additive (a deterministic
per-topic **seed** plus a **merge** at distillation; claim-vintage-expired;
hard-bounded with a fully deterministic truncation order; fresh supersedes
cached). Layer-1 (URL-keyed document cache) is unchanged; Layer-2 (per-holding
distilled findings) is the seed/merge layer; the audit records **seeded-vs-cold**.
**Option A** ruled: carried-holding staleness is an accepted cost — there is no
research-age force-include. **Extending the model to Trade Opportunities is
deferred.** Single-homed at `portfolio-analysis.md §Starting parameters` and
propagated across 9 files (incl. BUILD/INDEX). Seven Codex rounds to approval;
shipped and pushed as `ab82347`.

## Current state

Clean tree on `main` at `ab82347` (this handoff aside). No work in flight. The
redesign is **doc-only** — Step 6c is still stubbed, so it lands when the
research-loop slice is built. The build queue is unchanged by the detour:
completion block (Step-5 context loads + pre-flag + forensic producer; evidence
legs incl. FINRA/CBOE; 6a recall + checkpoint/resume + 6g validator; fund depth)
→ big run (watch-set v9 revision first) → Trade Opportunities → research loop +
refresh lane.

## Open questions

- **Scenario-differentiated priced-fund target formula** — undesigned; the
  shipped flat-driver form is the settled stopgap. Needs its ruling before the
  fund-depth group is planned. (carried)
- **Share-based action sizing** — ruled the only legal action numeric, unbuilt;
  nothing blocks on it. (carried)
- **Live-evidence caveat** — the sector-P/E walk-back's holiday warrant still
  rests on the 2026-07-16 verification, not re-probed. (carried)

## Where to start

Resume the `logic-flow-docs/portfolio-analysis-logic-flow.md` clarity walk from
**Step 6c onward** (the walk reached Step 6 / the work-list; its reuse sections
are now current with this session's redesign). Same posture: read each section,
surface confusions, apply clarity edits with the user, and ground any doubtful
claim against the canonical `docs/`.
