# Portfolio Analysis

Portfolio Analysis is a **local, on-demand job** that grades every holding in the user's portfolio and recommends an action for each, grounded in the holding's fundamentals, fresh web research, and the current Market Signal house view. It runs entirely on local models (see [local-models.md](local-models.md)) and sources holdings from Charles Schwab or manual import (see [schwab-integration.md](schwab-integration.md)).

It is deliberately **prescriptive** — it issues buy/trim/hold/sell-style actions and explicit price targets — which is the opposite of the Market Signal Report's no-buy/sell stance ([report-structure.md](report-structure.md)). The report forms the *house view*; this job applies that view to the specific positions the user holds.

## Triggering

The job is manual and runs in two user-controlled steps: **pull holdings**, then **run analysis**. Only one heavy local job runs at a time (Portfolio Analysis and Trade Opportunities share a single execution slot). While it runs it streams into the run tracker with **per-holding progress**, the same observability seam the report uses ([run-tracking.md](run-tracking.md)).

## The per-holding pipeline

Each holding is processed through a chain of distilled, schema-validated hand-offs (see [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)):

1. **Dossier assembly (deterministic, application layer).** The app builds the holding's evidence packet: the position itself (quantity, cost basis, market value, P/L); FMP fundamentals and financial statements; price history; the latest report's **Thesis, Investment Strategy, and Forward Outlook** sections (full prose) plus its summary metadata (`thesis_stance`, `forward_outlook_themes`, `key_risks`); vector-retrieved excerpts relevant to *this holding* from prior reports and from this job's own prior runs; and the **prior run's verdict for this holding**.
2. **Bounded web research** — the 122B reasoner in thinking mode plus the web tool ([web-research.md](web-research.md)). The model researches the company and its setup, request- and time-bounded, fail-soft.
3. **Distillation** — the 35B fast model condenses the research into a compact findings object, so the analysis stage reasons over evidence rather than over the research transcript.
4. **Analysis and grading** — the 122B reasoner in thinking mode produces the verdict over the dossier and findings.
5. **Continuity check.** The verdict is compared to the prior run's; any change in grade, action, or target must be justified by what materially changed. Output is firm and does not swing run-to-run absent hard supporting data (see [thesis-continuity.md](thesis-continuity.md)).

## The holding verdict

Each holding's output is a structured, schema-validated record:

- **Composite grade** (A–F) with transparent sub-scores — **quality**, **valuation**, **momentum**, and **risk** — so the letter is explainable, not a black box.
- **Action**, on a fixed ladder: **sell all → trim → hold → add → add aggressively**.
- **Horizon outlook** — separate short-, mid-, and long-term reads.
- **Price targets** — end-of-month and end-of-year, each a firm base case with a tight range.
- **Financial analysis** — a concise read of the company's financial health.
- **What changed** — the continuity diff against the prior run (or "new holding").

The fixed action ladder and grade vocabulary are load-bearing: like the report's fixed regime labels ([storage.md](storage.md)), they keep verdicts comparable across runs and prevent the model from retreating into hedged, non-comparable language.

## Portfolio roll-up

After the per-holding pass, a synthesis stage (122B) produces a **portfolio-level view**: concentration and sector/factor exposure, overall risk posture, and a cash stance — read against the report's house view. This is where the job answers "what does the portfolio as a whole look like, and how does it sit relative to the current market thesis," beyond the sum of individual holdings.

## Continuity and isolation

The job retains its most recent N runs. The prior run feeds the next (continuity above), and run results are embedded into the **Portfolio Analysis memory partition only** — its learnings are never read by the report or by Trade Opportunities, and it never reads theirs (see [local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)). This keeps holding-grading calibration cumulative within the job and uncontaminated by other jobs' context.

## Storage and display

Each run persists its per-holding verdicts and the portfolio roll-up; retention keeps the last N runs (parallel to report retention — [storage.md](storage.md)). The **Portfolio page** renders each holding's verdict — grade and sub-scores, action, outlook, targets, financials, and the what-changed line — alongside the portfolio roll-up (see [interface.md](interface.md)).

## Failure posture

The execution gate requires the local model daemon and roster ([local-models.md](local-models.md)) and available holdings ([schwab-integration.md](schwab-integration.md)); without them the job is blocked. Within a run, web research is fail-soft (thinner evidence, lower conviction), while a hard model or persistence failure fails the run, recorded like any other job ([scheduling.md](scheduling.md)).
