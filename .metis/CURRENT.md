# Current session handoff

## What happened

**Codex I4 landed** (`7d2f618`). The execution read pairs an actual only
against **ex-ante** guidance — published on or before the period end and
strictly before the period's *earliest* actual (the first time the actual
became public, not the actual selected) — the latest admissible revision
binding (range low over point at the same date, then confidence), the actual by
highest confidence then latest date, and a same-vintage conflict on either side
making the period **not comparable** rather than bound by persistence order;
dates parse from the ISO prefix and compare as dates; each miss records the
bound's and the actual's publication dates. Codex round 1 caught the policy
defeated one seam earlier — the dedup key (identity + role + period + source)
rejected a same-source revision as a duplicate — so the key now adds the parsed
publication date and the value, pinned through the production validator. The
6d prompt names `published_at` (the quoted page's own date, never the fetch
date). `PRE_PROFIT_PARAMETER_VERSION` → **`pre-profit-v3`**, `PROMPT_VERSION`
→ **`portfolio-v18`**; no grade, target, or floor stamp. Six plan rulings, one
reviewer round (four nits closed), two Codex rounds; fourteen tests. Lessons:
an order-independence test must run through the production validator, not the
read alone; a new pairing rule needs the *admission* key checked for the same
order-dependence. `BUILD.md` §What remains' count line bumped to I5–I13,
I15–I19 (user-authorized at session end).

## Current state

Nothing in flight; `main` at the session-end commit, tree clean, pushed. Queue
ahead of the run (record §Disposition): **Codex I5–I13, I15–I19** and the
**§A4 seed edge**, one finding per slice, a batch never mixing code and doc
findings. I18 and I19 are ruling items; I15's shape is ruled at its plan; I16
the required-`f64` audit; I17 the telemetry row pattern. Carried untouched
(unchanged): the cloud report job's unguarded `run_job` seam; the negative
composite yield; `progress.rs`'s poisonable terminal-leg locks; the `ok`
tracker row's dropped-count detail; `trade-opportunities-logic-flow.md:397`
"never sized"; `/api/tags` probes on the 600 s backstop; seed passes the whole
prior ledger per topic; 6g qualitative trips un-trip unless re-researched; an
IPv6-loopback wire test. Watch set: the attempt-2-prior line stays true (reads
back under `evidence-floor-v1` / `pre-profit-v1`; a mismatched trail is
refused by reason); records now stamp `portfolio-v18`. Named residual: FY
periods normalize to 12-31, so a non-December fiscal year leans on the
earliest-actual leg.

## Open questions

- Whether `INDEX.md`'s §Verification records row ("Codex I1–I18 additions")
  moves to I19, and whether INDEX gains rows for the in-quarter history
  admission (`portfolio-analysis.md` §Asset eligibility), the
  observation-excerpt contract (`portfolio-workflow.md` §Step 6e), and now the
  guidance vintage policy (`portfolio-analysis.md` §Starting parameters).
  User-run edits.

## Where to start

`/metis-session-start`, then `/metis-plan-task` **Codex I5** (the action
decision never receives the model arm's price targets — confirm it is the
first `### I` section with no `Resolved` line) and re-read every line anchor
and every pointer's owning heading first, checking whether a later slice
already closed it. Present assumptions and flags before implementing — the
user rules first. Keep the loop per finding (plan → implement → review → Codex
→ commit), record every Codex round, sweep `logic-flow-docs/` mirrors, and ask
of every fix what stamp it moves across the four axes (prompt content now
`portfolio-v18`, grade band, stored-target basis, floor rule
`evidence-floor-v3`; the overlay stamp `pre-profit-v3`). Do not launch or
propose the big run — the user names that session.
