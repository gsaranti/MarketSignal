
You are running an independent review of this project's `docs/` corpus — the
same review a prior agent already ran (originally three passes; the Market
Signal Report pass is retired — see below). This is the nineth run for you.
Finding something the prior rounds missed is the goal; independently confirming
an area clean is also a useful result. What you find, Claude Code will then fix.
This review loop will continue until no more issues are found. Therefore, you
can expect at some point to not find any issues in the docs. A review that returns
no issues is completely valid.

The prior round's findings have all been resolved, and the resolutions are
recorded in **`claude-code-fixes.md`** at the repo root — read it before
writing up any candidate finding, per the "Prior-round resolutions" section
below.

## Purpose

The `docs/` corpus is the **implementation-facing specification** for this app.
One job (the Market Signal Report) is built and shipped; the other two
(Portfolio Analysis, Trade Opportunities) are **designed, not built** — their
docs are the contract future implementation sessions will build from, so a
contradiction, an ambiguous contract, or a claim misaligned with the data-plan
reality becomes a real defect later. The review exists to catch exactly that
class of problem **in the spec**, before code.

The end goal, in the user's own words: these docs feed a per-task loop of
**plan → implement → review** (the Metis workflow), and this review exists to
make sure that when we do the metis task plan, task implementation, and task
review, the docs support the best possible code output. Do not be a perfectionist.
If you find something that really has no effect on the code implementation of
the job or the data quality of the job output, then really consider whether it needs
to be handled or not.

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
  **Review-complete** — run 6 returned this group clean (deep enrichment pass
  + light suite sweep, no finding survived), so it is out of scope below.
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

## The two passes and their weighting

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

**The former Pass 3 — Market Signal Report — is retired.** Run 6 executed it
in full (the enrichment-weighted deep pass plus the light report-suite sweep)
and no finding survived triage, so the report group is review-complete: do not
sweep the Market Signal Report docs for their own sake.
The shared docs stay fully in scope through their Portfolio / Trade
Opportunities sections, and if a Pass-1/2 finding directly implicates a report
doc (a shared contract both sides hold), citing it is still legitimate — the
retirement removes the standalone sweep, not the cross-reference.

## Method that worked (suggested execution, feel free adjust)

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

## Prior-round resolutions — use `claude-code-fixes.md` to triage a candidate finding

`claude-code-fixes.md` (repo root) records how every prior-round finding was
resolved: a disposition table naming each fixed contract's **canonical home**,
the findings that were **refuted** (with the resolving anchors), a list of
**deliberate designs** that can look like defects, the **drafted constants /
typed states** added on purpose, and the **named open items** that are
deliberately unresolved. Use it as the triage step for every candidate finding:

- **Match first.** Before writing a finding up, check whether it (or its
  underlying contract) appears in the file. If it does, go to the named
  canonical home and verify the doc actually holds the recorded contract —
  the notes are a map, not an authority; the docs remain the source of truth.
- **Doc holds the contract → drop the finding.** Re-flagging a resolved,
  refuted, deliberate, or named-open item is noise, not coverage.
- **Doc does not match the notes → that is a real finding.** A recorded fix
  that isn't actually in the doc, a canonical home that drifted from its
  pointers since, or a new contradiction the fix itself introduced all clear
  the bar — report those against the doc, citing the mismatch with the notes.
- **Drafted constants and deliberate designs are not omissions**, so don't
  flag their existence or their pending live verification (e.g. the Stooq
  symbol map is M5-gated by design) — but a *factual error inside one* (a
  wrong mapping, an internally inconsistent constant) still clears the bar.
- **Genuinely new ground stays fully in scope.** The file constrains only
  re-flagging; it never lowers the bar for something the prior rounds missed.

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
repo root. Order findings most-severe first, grouped by pass. For each finding
give:

1. a one-sentence statement of the defect;
2. every involved location (`file.md §Section`, quote the conflicting
   clauses);
3. why it is material (what breaks at implementation or run time);
4. a concrete recommended fix, including which doc is the canonical home.

Close with a per-pass summary of what you checked and found clean, so a clean
area is distinguishable from an unchecked one.
