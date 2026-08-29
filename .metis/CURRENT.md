# Current session handoff

## What happened

**Codex I1 landed** (`cad7c0a`) — the usable-quote floor: `engine::usable_price`
(finite, strictly positive) at the FMP quote parse and both engine floors, the
fund analog falling to a usable NAV, the ledger `Price` series unevaluable on
an unusable print. Three Codex rounds. Round 1 hardened `nav_premium_read`
and — by ruling, over my pushback that the standing three stamp axes don't
cover a floor change — added a dedicated **`engine::EVIDENCE_FLOOR_VERSION`**
(`evidence-floor-v2`) on the checkpoint header and every audit record. Round
2 caught that the new required fields would have loud-skipped **attempt 2's
persisted run** — the dev store's one run, the big run's diff / carry
baseline (verified by copying the dev SQLite) — so both fields default to
`evidence-floor-v1`, and the CEF gap cause moved onto usability. Lessons: a
new field on a persisted struct follows that struct's `#[serde(default)]`
convention with the pre-field meaning named (the avoid-premature-compat rule
is about cutting fields that never take a non-default value, not about new
fields over live records); check the dev store before grading a compat
finding. "Queue it as I14" was a slip — I14 is taken — so the general
resume-across-a-rebuild question is **I18**; **I19 is the next free item**.
BUILD / INDEX bumped at session-end (user-authorized): I1–I18, the usability
standing constraint, the stamp in the version-constant list, one INDEX row.

## Current state

Nothing in flight; `main` at `cad7c0a`, tree clean, pushed. Queue ahead of
the run (record §Disposition): **Codex I2–I18** and the **§A4 seed edge**,
one finding per slice, a batch never mixing code and doc findings. **I18 is a
ruling item**, not a code slice — a build-identity stamp on the trail
(refuse resume across any rebuild) vs. the stamp axes as the contract —
plan it as a ruling. I15's shape (wire / retire) is ruled at its own plan;
I16 is the required-`f64` audit with a store round-trip regression; I17 (the
run-level checkpoint counters' resume over-count) follows the telemetry's
row pattern. Carried untouched: the cloud report job's unguarded `run_job`
seam; the negative composite yield; `progress.rs`'s poisonable terminal-leg
locks; the `ok` tracker row's dropped-count detail;
`trade-opportunities-logic-flow.md:397` "never sized" (true for the unbuilt
TO job); `/api/tags` probes on the 600 s backstop; seed passes the whole
prior ledger per topic; 6g qualitative trips un-trip unless re-researched;
an IPv6-loopback wire test (a candidate, not queued). The watch set's
attempt-2-prior line stays true — the run reads back under
`evidence-floor-v1`.

## Open questions

- None new. `BUILD.md` / `INDEX.md` were bumped at this session-end; the
  three `.metis/` files are left uncommitted for the user's `metis:` commit.

## Where to start

`/metis-session-start`, then `/metis-plan-task` **Codex I2** (the first
unresolved item — confirm by grepping `### I` in the record for the first
section with no `Resolved` line) and re-read every line anchor and every
pointer's owning heading first, checking whether a later slice already
closed it. Present the plan's assumptions and flags before implementing —
the user rules on them first. Keep the loop per finding (plan → implement →
review → Codex → commit), mark it resolved in the record with every Codex
round named, sweep `logic-flow-docs/` mirrors, and ask of every fix what
stamp it moves — now four axes: a prompt-content change bumps
`PROMPT_VERSION` with its history paragraph and the watch-set stamp line; a
grade-band change appends a `GRADE_PARAMETER_HISTORY` row; a stored-target
basis change bumps the targets stamp; a floor-rule change bumps
`EVIDENCE_FLOOR_VERSION`. Do not launch or propose the big run — the user
names that session.
