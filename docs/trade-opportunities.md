# Trade Opportunities

Trade Opportunities is a **local, on-demand job** that surfaces investment ideas through deep web research, organized across a fixed risk-by-horizon matrix and grounded in the current Market Signal house view. Like Portfolio Analysis it runs entirely on local models (see [local-models.md](local-models.md)) and is **prescriptive and firm**; unlike it, it is **not tied to current holdings** — its purpose is to discover new opportunities. It still **requires a connected Schwab account**, whose option chains supply the per-candidate options-activity signal (see [schwab-integration.md](schwab-integration.md)).

## Triggering

The job is manual and runs in the run tracker with per-cell progress. It runs under the same single global run slot as the report and Portfolio Analysis — only one run at a time across the app (see [local-models.md §Failure posture](local-models.md#failure-posture), [run-tracking.md](run-tracking.md)).

## The opportunity space

Output is organized as a **3×3 matrix**: three **risk tiers** (high / medium / low) × three **horizons** (short / mid / long term) = nine cells, each holding a small set of opportunities (or none, when nothing qualifies). The user sees high-, medium-, and low-risk sections, each containing short-, mid-, and long-term ideas. **Risk-tier assignment is deterministic** — derived by rule from measurable inputs (profitability, market cap, liquidity, volatility, leverage, drawdown, and event exposure), not a label the model picks — so the same asset lands in the same tier run to run. The fixed matrix is what makes the output comparable across runs and forces breadth — the job must consider every risk/horizon combination rather than clustering on whatever is topical.

## The pipeline

1. **Framing (deterministic + the 35B fast model).** The app assembles the house-view context — loaded deterministically as the latest report's Thesis, Investment Strategy, and Forward Outlook plus recent summaries (not vector-searched — see [local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)) — together with the **prior run's opportunity set** (vector-retrieved from this job's own partition). Research directions are set per matrix cell. Current holdings are deliberately **not** part of framing (see [§Holdings cross-reference](#holdings-cross-reference)), so discovery and selection stay independent of the account.
2. **Candidate generation.** SearXNG is not a market screener, so candidates come from two deterministic feeders: **FMP signals** (the movers, valuation extremes, and earnings the app already gathers — [data-sources.md](data-sources.md); a broader market-cap / liquidity universe would add FMP's screener endpoint, whose tier and per-run call budget are an implementation detail to settle) and **names surfaced by research**. The combined set is deduped, sanity-filtered for tradability, and tagged with the data used to rank it.
3. **Deep research** — the 122B reasoner (thinking mode) plus the web tool, running the bounded research loop per candidate ([web-research.md](web-research.md)), fail-soft. The agenda builds the opportunity case: the thesis and its catalyst, and the bull / bear; **the driving narrative and market sentiment** — how much the setup rides emotion about *what might come* versus present fundamentals, and how durable that story is; **forward opportunity and thematic tailwinds** — the future opportunity set and how the candidate maps onto dominant market themes (e.g. a photonics name riding the AI-buildout narrative); and how it corroborates its deterministically-assigned risk tier and horizon.
4. **Distillation** — the 35B model consolidates the loop's findings into candidate summaries.
5. **Selection and authoring** — each candidate's metrics, **risk tier, and options-activity signal** (put/call + IV/skew from Schwab chains) are computed deterministically (the same financial-analysis engine Portfolio Analysis uses — see [portfolio-analysis.md](portfolio-analysis.md)); the 122B reasoner then interprets those, selects, and writes up the opportunities for each cell. **A cell may return no opportunity** when nothing qualifies — empty cells are honest, not failures, and the matrix never pads itself to fill them.
6. **Continuity check.** Prior opportunities are carried forward with an updated status; additions and removals must be justified by what changed (see [thesis-continuity.md](thesis-continuity.md)).

## The opportunity

Each opportunity is a structured, schema-validated record:

- **asset / ticker**
- **directional thesis** — firm and specific
- **catalyst** — why now
- **horizon** — short / mid / long (matches its cell)
- **risk tier** — high / medium / low, assigned deterministically by rule (matches its cell)
- **conviction** level
- **entry consideration**
- **status** — `new`, `still-valid`, `played-out`, or `invalidated`, for carry-forward across runs

The fixed risk/horizon/status vocabularies keep the matrix stable and the list evolving rather than churning — an idea persists with an updated status instead of silently reappearing or vanishing.

## Holdings cross-reference

After selection, a **deterministic post-step** flags any opportunity that overlaps the user's current holdings (owned / not-owned), so the two features cohere — an idea you already hold is surfaced as such. Crucially this runs *after* candidate discovery and selection, so holdings never influence which opportunities are found or chosen; it reads only the holdings list, never the Portfolio Analysis memory partition. Trade Opportunities therefore stays genuinely independent of the account.

## Continuity and isolation

The job retains its most recent N runs, feeds the prior run into the next, and embeds results into the **Trade Opportunities memory partition only** — isolated from the report's and Portfolio Analysis's memory (see [local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)). Output is firm and does not churn between runs absent hard data.

## Storage and display

Each run persists its matrix of opportunities together with an **audit record** (the report(s) and sources used with retrieval timestamps, the screening inputs, the model ids and quantizations, and the prompt/schema version); retention keeps the last N runs ([storage.md](storage.md)). The **Trade Opportunities page** renders the 3×3 matrix, each cell listing its opportunities with thesis, catalyst, conviction, entry consideration, and status (see [interface.md](interface.md)).

## Failure posture

The execution gate requires the local model daemon and roster ([local-models.md](local-models.md)) **and a connected Schwab account** (for the options-activity signal — see [schwab-integration.md](schwab-integration.md)); a missing or lapsed connection blocks the job. Web research within a run is fail-soft, while a hard model or persistence failure fails the run ([scheduling.md](scheduling.md)).
