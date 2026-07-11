# Codex brief — full docs-corpus review (3 passes)

You are running an independent review of this project's `docs/` corpus — the
same three-pass review a prior agent already ran. Finding something the prior
rounds missed is the goal; independently confirming an area clean is also a useful
result.

## Purpose

The `docs/` corpus is the **implementation-facing specification** for this app.
One job (the Market Signal Report) is built and shipped; the other two
(Portfolio Analysis, Trade Opportunities) are **designed, not built** — their
docs are the contract future implementation sessions will build from, so a
contradiction, an ambiguous contract, or a claim misaligned with the data-plan
reality becomes a real defect later. The review exists to catch exactly that
class of problem **in the spec**, before code.

The end goal, in the user's own words: these docs feed a per-task loop of
**plan → implement → review** (the Metis workflow), and the review exists "to
make sure that when we do the metis task plan, task implementation, and task
review, the docs support the best possible code output. Do not be a perfectionist.
If you find something that really has no effect on the code implementation of
the job or the quality of the job output, then really consider whether it needs
to be handled or not".

This is **not a strategy re-audit** — do not audit the jobs' investment
strategies or attempt to make them better. The investment logic, gate designs,
and methodology decisions are settled (each converged through prior strategy
audits plus external review rounds). **The one exception:** if a documented
piece of strategy is *glaringly wrong* in a way that will lead to worse or
incorrect job results, flag it — that clears the bar; a "this could be
better" strategy opinion does not.

## The corpus

24 files in `docs/`. Three job groups plus shared docs:

- **Market Signal Report** — `report-workflow.md`, `report-structure.md`,
  `agents.md`, `analyst-skills.md`, `thesis-continuity.md`,
  `research-documents.md`, `export.md`, `run-tracking.md`, `scheduling.md`.
- **Portfolio Analysis** — `portfolio-analysis.md`, `portfolio-workflow.md`.
- **Trade Opportunities** — `trade-opportunities.md`,
  `trade-opportunities-workflow.md`.
- **Shared** — `data-sources.md` (the provider catalog, the FMP paid-plan tier
  audit, and per-job endpoint tables), `storage.md`, `configuration.md`,
  `interface.md`, `web-research.md`, `local-models.md`,
  `local-model-operations.md`, `schwab-integration.md`, `data-portability.md`,
  `overview.md`, `README.md`.

Build status lives in `.metis/BUILD.md` (read-only for you; do not edit
anything under `.metis/`). The docs deliberately describe designed and built
features **without distinction** — that is a convention, not a defect.

## The review contract (binds every pass)

1. **Cold read.** Read the pass's primary docs in full, plus that job's
   sections of the shared docs. Do not skim from summaries.
2. **Contradictions.** Any two passages that specify the same contract
   differently — across docs or within one — are a finding. This includes
   quiet drift: an enumeration in a design doc that no longer matches the
   endpoint tables, a threshold stated with two values, a step described with
   two different behaviors.
3. **Dangerous duplication / single-homing.** Each contract should be
   specified **once**, in its canonical home doc; other docs get one-line
   pointers. A pointer restating a clause is fine; a second full specification
   is a finding (it will drift). Challenge cross-links: does the link point at
   the canonical home?
4. **Tier-audit alignment.** Every FMP endpoint any doc claims a job calls —
   including *planned* endpoints — must appear in
   `data-sources.md §FMP — current paid-plan tier audit` in a compatible
   bucket, with its usage compatible with the recorded constraint (exchange
   sets, annual-only periods, history caps). Prior rounds caught real
   misalignments in exactly this class.
5. **Model-call contract completeness.** Every model call / agent stage must
   be fully specified: the step's **Type** tag, and for each call the model,
   what the prompt contains, what it returns, and how the app validates the
   result. For the two **local-model** workflows, every call also carries a
   **thinking-mode designation** (the Mode legend / Ollama issue #14645 caveat
   is deliberate — do not flag it as an error; flag a call that *lacks* a
   designation).
6. **Materiality filter — apply it to every finding.** A finding must be
   implementation-affecting or job-result-affecting: would it change the code
   an implementer writes, or degrade the quality/correctness of the job's
   output? If it affects neither, really consider whether it needs to be
   handled at all — wording taste, style, and harmless redundancy do not
   clear the bar. If you report a nit anyway, label it a nit.
7. **Findings first, no edits.** Report findings with concrete recommended
   fixes. The user approves which to apply. Do not modify any file.

## The three passes and their weighting

**Pass 1 — Portfolio Analysis (full-contract deep read).** Cold read of
`portfolio-analysis.md` + `portfolio-workflow.md` end to end, plus the
Portfolio sections/rows of the shared docs (data-sources' Portfolio endpoint
table, storage's Local Analysis Suite Storage, configuration, interface,
web-research, local-models, schwab-integration). The feature is designed-not-
built, so everything is in scope: the intrinsic-verdict discriminated union,
the ledger/quick-check/selective-re-analysis semantics, outcome learning, the
fund path, and every model-call contract in the workflow.

**Pass 2 — Trade Opportunities (full-contract deep read).** Same contract as
pass 1: `trade-opportunities.md` + `trade-opportunities-workflow.md` end to
end, plus the TO sections of the shared docs (the TO endpoint tables and their
three cardinality bands, the discovery feeders, the shadow ledger and outcome
learning, the opportunity graph, the DTO/ATO lifecycle).

**Pass 3 — Market Signal Report (enrichment-weighted).** The report is built
and shipped, so its as-built prose is settled — weight accordingly:

- **Deep (full contract):** the *planned report enrichment* —
  `data-sources.md §Planned report enrichment` plus the report endpoint
  table's planned-paid rows, `report-workflow.md §Step 3` and `§Step 16`
  (where the new signals enter the packet and the prompts), and
  `storage.md §Baseline Snapshots` (where the enriched scan serializes).
  Check: internal consistency of the three enrichment signals across those
  homes; every planned endpoint against the tier audit; the stated exclusions
  holding (all three engine-derived, outside the level-delta engine; breadth
  ruled out); window sizing consistent with the cadence-honest contract
  (windows sized to the actual elapsed interval, clamped where stated).
- **Light (checklist sweep only):** the rest of the report group — for the two
  promoted classes below. Do not re-litigate settled as-built prose beyond
  those; flag anything else only if it is glaring **and** job-result-affecting.

## Checklist classes promoted from earlier passes

Two recurring defect classes emerged across the prior rounds — sweep every
pass for them explicitly:

1. **Design-doc enumerations vs the data-sources endpoint tables.** Any list
   of feeds, feeders, signals, or series in a job doc must match the job's
   endpoint table (and the tier audit) exactly — no extra source, no missing
   row.
2. **Every model-call / agent-stage contract fully specified** — see rule 5.

## Method that worked (suggested execution)

- Read the pass's primary docs in full first, then the shared docs' relevant
  sections, keeping the endpoint tables and tier audit at hand as the
  cross-reference base.
- For every data claim, resolve it to a table row or series ID; for every
  contract stated twice, decide which home is canonical and whether the twin
  is a pointer or a drifting duplicate.
- Grep for concept names across the whole corpus before calling something
  single-homed or missing — dense doc prose splits identifiers across lines,
  so grep fragments, not just full phrases.
- Finish with a **link/anchor sweep**: every relative `file.md#anchor` link in
  `docs/` must resolve (GitHub-style anchor slugs). Expected result: 0 broken.

## Deliberate decisions — do not flag these as defects

- The #14645 thinking-mode caveat in the local workflows' Mode legends.
- Analyst skills are forcing-function-only (verdicts never parsed/persisted).
- GDELT dropped from the suite; TO discovery is SearXNG-only (no Tavily);
  Tavily is per-candidate fallback only.
- Off-plan FMP endpoints (`*-bulk`, transcripts, 13F, fund-holdings,
  press-releases) with recorded fallbacks — the audit is live-verified.
- True index breadth ruled out for the report (movers stay the proxy).
- HTML never persisted; stored filenames carry `-<id8>`, exports drop it.
- "Amended YYYY-MM-DD" notes are historical records, not staleness.
- Docs describing designed-not-built features without distinction.

## Output

Write your findings as a single Markdown report to `codex-review.md` at the
repo root (gitignored; overwrite whatever is there). Order findings
most-severe first, grouped by pass. For each finding give:

1. a one-sentence statement of the defect;
2. every involved location (`file.md §Section`, quote the conflicting
   clauses);
3. why it is material (what breaks at implementation or run time);
4. a concrete recommended fix, including which doc is the canonical home.

Close with a per-pass summary of what you checked and found clean, so a clean
area is distinguishable from an unchecked one.
