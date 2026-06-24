# Trade Opportunities

Trade Opportunities is a **local, on-demand job** that surfaces investment ideas through deep web research, organized across a fixed risk-by-horizon matrix and grounded in the current Market Signal house view. Like Portfolio Analysis it runs entirely on local models (see [local-models.md](local-models.md)) and is **prescriptive and firm**; unlike it, it is **not tied to current holdings** — its purpose is to discover new opportunities.

## Triggering

The job is manual and runs in the run tracker with per-cell progress. It shares the single heavy-local-job execution slot with Portfolio Analysis ([run-tracking.md](run-tracking.md)).

## The opportunity space

Output is organized as a **3×3 matrix**: three **risk tiers** (high / medium / low) × three **horizons** (short / mid / long term) = nine cells, each holding a small set of opportunities. The user sees high-, medium-, and low-risk sections, each containing short-, mid-, and long-term ideas. The fixed matrix is what makes the output comparable across runs and forces breadth — the job must consider every risk/horizon combination rather than clustering on whatever is topical.

## The pipeline

1. **Framing (deterministic + the 35B fast model).** The app assembles the house-view context — the latest report's Thesis, Investment Strategy, and Forward Outlook plus its summary metadata — together with the **prior run's opportunity set** and, optionally, the current holdings list for cross-reference. Research directions are set per matrix cell.
2. **Deep research** — the 122B reasoner in thinking mode plus the web tool ([web-research.md](web-research.md)), bounded per direction and fail-soft.
3. **Distillation** — the 35B model condenses findings to candidate summaries.
4. **Selection and authoring** — the 122B reasoner selects and writes up the opportunities for each cell.
5. **Continuity check.** Prior opportunities are carried forward with an updated status; additions and removals must be justified by what changed (see [thesis-continuity.md](thesis-continuity.md)).

## The opportunity

Each opportunity is a structured, schema-validated record:

- **asset / ticker**
- **directional thesis** — firm and specific
- **catalyst** — why now
- **horizon** — short / mid / long (matches its cell)
- **risk tier** — high / medium / low (matches its cell)
- **conviction** level
- **entry consideration**
- **status** — `new`, `still-valid`, `played-out`, or `invalidated`, for carry-forward across runs

The fixed risk/horizon/status vocabularies keep the matrix stable and the list evolving rather than churning — an idea persists with an updated status instead of silently reappearing or vanishing.

## Holdings cross-reference

The job optionally flags opportunities that **overlap the user's current holdings**, so the two features cohere (an idea you already own is surfaced as such). This reads only the **holdings list** — shared input data — and never the Portfolio Analysis memory partition, so the memory-isolation rule (below) is preserved.

## Continuity and isolation

The job retains its most recent N runs, feeds the prior run into the next, and embeds results into the **Trade Opportunities memory partition only** — isolated from the report's and Portfolio Analysis's memory (see [local-models.md §Run history and continuity](local-models.md#run-history-and-continuity)). Output is firm and does not churn between runs absent hard data.

## Storage and display

Each run persists its matrix of opportunities; retention keeps the last N runs ([storage.md](storage.md)). The **Trade Opportunities page** renders the 3×3 matrix, each cell listing its opportunities with thesis, catalyst, conviction, entry consideration, and status (see [interface.md](interface.md)).

## Failure posture

The execution gate requires the local model daemon and roster ([local-models.md](local-models.md)); web research within a run is fail-soft, while a hard model or persistence failure fails the run ([scheduling.md](scheduling.md)).
