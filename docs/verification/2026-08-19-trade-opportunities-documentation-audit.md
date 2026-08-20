# Trade Opportunities documentation audit — 2026-08-19

## Verdict

**The Trade Opportunities documentation is not yet aligned closely enough for clean implementation.**
The core job shape is coherent across the human-readable logic flow and the technical documents: DTO discovers and maintains the full matrix, ATO audits selected live opportunities, the deterministic engine and model author separate arms, model tier × horizon places each card, either arm may satisfy the entry gate, and only a qualifying deep pass may rewrite or archive a carried opportunity.
The audit nevertheless found 23 contract conflicts or material ambiguities that could cause independent implementers to build different call graphs, persisted schemas, lifecycle behavior, source cardinalities, job-status behavior, or user-facing output.
It also found five shared specification gaps that are stated consistently but are not executable as written, plus eight deterministic rules that the logic flow already marks explicitly as not yet drafted.

No code was reviewed because the job has not been developed.
No existing documentation or code was changed.
This file is the only artifact added by the audit.

## Scope and authority

The primary comparison was between `logic-flow-docs/trade-opportunities-logic-flow.md` and the two owning technical documents, `docs/trade-opportunities.md` and `docs/trade-opportunities-workflow.md`.
The audit also traced every live top-level technical document that names Trade Opportunities or owns a shared contract it consumes: `README.md`, `overview.md`, `configuration.md`, `data-sources.md`, `storage.md`, `data-portability.md`, `interface.md`, `scheduling.md`, `run-tracking.md`, `local-models.md`, `local-model-operations.md`, `web-research.md`, `schwab-integration.md`, `portfolio-analysis.md`, `portfolio-workflow.md`, `report-structure.md`, `report-workflow.md`, and `thesis-continuity.md`.
Dated files already under `docs/verification/` were treated as historical evidence, not as live job contracts.
The `.metis/` handoff was used only to orient the audit and was not treated as a technical source of truth.

The logic flow was not presumed automatically correct merely because it is the human-readable document.
Where documents conflict, the likely intended contract below is inferred from the most specific owning section and the surrounding invariants, but the documents still require an explicit ruling and synchronized correction.
Technical detail that is safely delegated by the logic flow is not a finding merely because the logic flow omits it.

## Contract conflicts and material ambiguities

### H1 — Step 3b does not define one coherent route-research and card-formation call graph

The logic flow isolates every topic conversation, then runs one non-tooling card-formation call over the route's accumulated topic findings and evidence ledger at `logic-flow-docs/trade-opportunities-logic-flow.md:560-568`.
The shared research-loop contract also says topics meet only at that downstream consolidating call at `logic-flow-docs/trade-opportunities-logic-flow.md:316-320`.
The technical workflow's exact call block instead describes a tool-using **Hypothesis research & exposed-name discovery** call given “one route's questions at a time” and returning hypothesis cards directly at `docs/trade-opportunities-workflow.md:190-205`.
That block specifies the heavy-route reduce input but never specifies the ordinary route-level card-formation call's aggregate input, and it omits the route's source-strategy rubric even though `docs/trade-opportunities-workflow.md:138-140` makes that rubric part of every route.
An implementer cannot tell whether topic calls, a route conversation, or a separate post-research call authors the cards.

**Required alignment:** define the normal and heavy call graph explicitly, including which call may use web tools, which call authors cards, the ordinary aggregate payload, the heavy substitute payload, and the route source strategy carried into each topic.

### H2 — The shadow ledger requires a Step-5g digest for decisions made before Step 5g

`docs/trade-opportunities.md:530-536` and `docs/storage.md:203-206` enumerate gate rejects, abstentions, budget or quota deferrals, dedup substitutes, and retirements, then require **every** episode to carry the model's Step-5g digest.
Budget and quota deferrals occur in Step 4 at `logic-flow-docs/trade-opportunities-logic-flow.md:650-662`.
Watchlist retirements occur in Step 3c at `logic-flow-docs/trade-opportunities-logic-flow.md:612-624`.
Step 5g does not begin until `logic-flow-docs/trade-opportunities-logic-flow.md:1005`, so those earlier decisions cannot have that record.
The logic flow correctly requires the digest for Step-5h holdouts at `logic-flow-docs/trade-opportunities-logic-flow.md:1126-1131`, and Step-6 dedup peers can also carry it because they passed through Step 5g.

**Required alignment:** make the digest optional and class-specific, or require it only for post-5g turn-aways and define a different pre-5g context record for deferrals and retirements.

### H3 — Fresh archive re-entry both rejects and consumes the old lifecycle

`docs/trade-opportunities.md:585-588` says a rediscovered archived ticker is a fresh start and that none of the archived record influences the new opportunity.
The same sentence at line 586 then says the old since-flagged read is fed to Step 5g cap-only “as for any carried name.”
The logic flow treats an archived match as a debut with nothing carried at `logic-flow-docs/trade-opportunities-logic-flow.md:645-647`, gives re-entries no continuity framing at lines 750-756, limits since-flagged context to carried names at lines 872-875, and repeats the no-influence rule at lines 1228-1232.
Feeding the departed lifecycle's price path into the new call would violate the lifecycle boundary and could anchor a genuinely new thesis to the old one.

**Required alignment:** remove old-lifecycle since-flagged context from a re-entry, or explicitly abandon the fresh-start invariant and define the cross-lifecycle data that may carry.

### H4 — The proposed status enum has no valid transition matrix

Step 5g may propose `new`, `still-valid`, or `invalidated` for every candidate at `logic-flow-docs/trade-opportunities-logic-flow.md:1037-1049` and `docs/trade-opportunities-workflow.md:538-545`.
Step 5h defines only carried-name outcomes at `logic-flow-docs/trade-opportunities-logic-flow.md:1105-1108`.
No document states that a debut must return `new`, that a carry may return only `still-valid` or `invalidated`, or what happens when the model returns an origin-incompatible value.
A carry proposed as `new` could reset lifecycle identity or `became_opportunity_at`, while a debut proposed as `invalidated` has no defined archive or shadow destination.

**Required alignment:** add an app-owned transition table by candidate origin, define whether invalid combinations fail schema or normalize, and state how the effective status affects lifecycle identity, matrix admission, the archive, and the shadow ledger.

### H5 — The durable-store inventory and ownership disagree

The logic flow defines six durable structures at `logic-flow-docs/trade-opportunities-logic-flow.md:1262-1271`: the run and matrix, opportunity graph, discovery-coverage ledger, archive, shadow ledger, and picked-episode store.
`docs/storage.md:184-212` independently defines the same six structures.
`docs/trade-opportunities-workflow.md:710-717` instead says the run audit record carries the shadow ledger, enumerates the graph, coverage ledger, and archive as peer stores, and omits the independent picked-episode store.
`docs/configuration.md:189-191` says only the validated matrix and opportunity graph carry forward through storage.
`docs/data-portability.md:194-198` says every new durable local-suite store joins the archive format but omits the discovery-coverage ledger from its Trade Opportunities list.
These variants would couple lifecycle data to run retention, omit stores, or produce incomplete import and export formats.

**Required alignment:** publish one canonical six-store inventory, state which structures are independent of run retention, and make the workflow, configuration, storage, retention, and portability lists exact consumers of that inventory.

### H6 — The entry-stamped sector identity is both frozen and refreshed

`docs/storage.md:191` calls the sector identity entry-stamped but says it is refreshed at each deep pass.
The outcome contract freezes the sector label and resolved benchmark at entry and forbids label-time reclassification at `logic-flow-docs/trade-opportunities-logic-flow.md:1210-1215`.
The picked episode persists that entry snapshot independently of later state at `logic-flow-docs/trade-opportunities-logic-flow.md:1223`.
Refreshing the same field would change the benchmark basis within one lifecycle and make relative outcomes across windows non-comparable.

**Required alignment:** keep the entry stamp immutable, and use a separately named current-sector field if deep passes need a current classification.

### H7 — The shared Portfolio contract reduces the hard-trigger outcome to debut exclusion

`docs/portfolio-analysis.md:487-491` says Trade Opportunities' hard-trigger outcome is exclusion at an entry gate.
The Trade Opportunities contract has two origin-sensitive outcomes at `logic-flow-docs/trade-opportunities-logic-flow.md:1100-1107`: a debut is excluded, while a carried opportunity is app-forced to `invalidated` and archived with a status-override divergence.
The shared statement erases the only app-forced archival path for a live pick.

**Required alignment:** state both Trade Opportunities outcomes before explaining how Portfolio translates the same trigger for an owned holding.

### H8 — SEC XBRL has incompatible maintenance cardinality

`docs/trade-opportunities-workflow.md:219-226` lets a filing-class watchlist read refresh from FMP statements or SEC XBRL.
The logic flow likewise includes SEC XBRL facts in the filing-cadence rider at `logic-flow-docs/trade-opportunities-logic-flow.md:601-605`.
The endpoint owner says the maintenance rider is FMP-only and keeps SEC submissions and company facts per-candidate at `docs/data-sources.md:704-705` and `docs/data-sources.md:818-819`.
The difference changes both the cheap-sweep call budget and what a filing-class condition promises it can refresh.

**Required alignment:** decide whether maintenance may call SEC, then make the resolution contract and endpoint cardinality table match that decision.

### H9 — Trade Opportunities has no consistent execution identity in shared status and UI contracts

The logic flow and configuration call DTO and ATO two jobs at `logic-flow-docs/trade-opportunities-logic-flow.md:10-18`, `logic-flow-docs/trade-opportunities-logic-flow.md:1314-1316`, and `docs/configuration.md:165-176`.
`docs/scheduling.md:3-10` and `docs/data-sources.md:13-17` count Trade Opportunities as one on-demand job and describe only discovery.
The more consequential problem is the supposedly exhaustive status mapping: `docs/scheduling.md:80-92` gives section-scoped `job_type` behavior only for Report and Portfolio, while `docs/interface.md:73-80` says every non-Portfolio section reports the report job.
`docs/run-tracking.md:3-9` names Report and Portfolio page replacement but omits the Trade Opportunities page, whereas `logic-flow-docs/trade-opportunities-logic-flow.md:1306-1307` requires the tracker to replace it while DTO or ATO runs.

**Required alignment:** decide whether DTO and ATO are distinct `job_type` values or modes under one Trade Opportunities identity, decide whether Quick and Deep are distinct audit modes or job types, and define page ownership, last-run and failure stamps, history aggregation, tracker placement, and global-running behavior for each.

### M1 — Source-budget prose places per-symbol scoring at two different funnel boundaries

`docs/data-sources.md:106-108`, `docs/data-sources.md:124-129`, and `docs/data-sources.md:712-714` describe `company-screener` followed by per-symbol scoring on a screener-narrowed longlist.
The owning cardinality section at `docs/data-sources.md:686-705` says discovery performs no rich pre-scoring and that per-candidate computation happens only after the discovery-breadth budget narrows the funnel.
The logic flow places the composite at Step 5c over the Step-4 slate at `logic-flow-docs/trade-opportunities-logic-flow.md:650-671` and `logic-flow-docs/trade-opportunities-logic-flow.md:788-811`.
“Screener-narrowed longlist” can therefore be read either as the entire stratified screener output or as the budgeted candidate slate, with very different request counts.

**Required alignment:** replace the legacy wording with the exact boundary: the screener stratifies and generates, Step 4 selects the budgeted slate, and only that slate spends the rich per-symbol surface at Step 5.

### M2 — Attention-warning clearing drops the inconclusive-refresh exception

Broad summaries say any or the next deep pass clears the warning at `logic-flow-docs/trade-opportunities-logic-flow.md:167-169`, `docs/trade-opportunities.md:389-396`, `docs/trade-opportunities-workflow.md:659-661`, and `docs/storage.md:191`.
The detailed evidence-floor rule says a carried deep re-read that remains below the floor is inconclusive and explicitly does not clear the warning at `logic-flow-docs/trade-opportunities-logic-flow.md:1117-1119`, `docs/trade-opportunities.md:295-297`, and `docs/trade-opportunities-workflow.md:585-586`.
ATO's detailed rule agrees at `logic-flow-docs/trade-opportunities-logic-flow.md:1371-1376` and `docs/trade-opportunities-workflow.md:774-776`.

**Required alignment:** use “the next successful, floor-clearing deep pass” everywhere.

### M3 — Quick Audit outcome-label ownership is unresolved

`docs/trade-opportunities.md:463-467` says an ATO audit refreshes outcome labels for the names it touches without limiting the statement to Deep Audit.
The logic-flow Quick Audit runs the cheap re-derivation and Step 9's deterministic persistence leg only at `logic-flow-docs/trade-opportunities-logic-flow.md:1330-1354`.
The logic flow mentions touched-name label refresh only under Deep Audit at `logic-flow-docs/trade-opportunities-logic-flow.md:1382-1384`.
The technical workflow's ATO reduction at `docs/trade-opportunities-workflow.md:779-781` also does not assign an outcome-learning subpass to Quick Audit.

**Required alignment:** decide whether Quick Audit may mature touched picked and shadow episodes; if it may, add its outcome subpass, benchmark-bar inputs, store updates, and pending or unscorable behavior to both workflows.

### M4 — The exact Step-5g prompt omits inputs that other sections say feed Step 5g

The dossier derives `continuity_weight` specifically to frame Step 5g at `docs/trade-opportunities-workflow.md:346-353`.
The workflow also says `validated_leading_indicator` remains evidence for Step 5g at `docs/trade-opportunities-workflow.md:518-520`.
The logic flow's exact input list includes the prior record framed by `continuity_weight`, the validated leading indicator, and any `technology_read` at `logic-flow-docs/trade-opportunities-logic-flow.md:1018-1022`.
The technical workflow's exact prompt block at `docs/trade-opportunities-workflow.md:524-536` says only “any prior opportunity record” and does not name the framing weight, validated indicator, or typed technology read.
Because the block presents itself as the exact implementable prompt, those omissions are not safely delegated.

**Required alignment:** make the two exact input inventories identical and state whether `technology_read` is a first-class input or merely nested in a surfacing record.

### M5 — User-facing forward-outlook ownership is ambiguous inside the core design document

`docs/trade-opportunities.md:361-368` labels the engine base case and range as the user-facing forward outlook.
The same document says the model arm headlines the matrix card and drives the default List-view target and sort at `docs/trade-opportunities.md:650-666`.
The workflow and logic flow agree with the latter at `docs/trade-opportunities-workflow.md:742-748` and `logic-flow-docs/trade-opportunities-logic-flow.md:1294-1301`.
The engine values are still user-visible on expand and in paired List-view columns, so the conflict is specifically about the primary field and label, not whether the engine arm is displayed at all.

**Required alignment:** call the model arm the card's forward-outlook headline and describe the engine arm as the disclosed baseline on expand and in paired columns.

### M6 — The archive storage schema omits admission provenance

The core design includes admission provenance on an archived card at `docs/trade-opportunities.md:579-583`.
The logic flow includes it in the frozen archive snapshot at `logic-flow-docs/trade-opportunities-logic-flow.md:1228-1232`.
`docs/storage.md:201-202` lists the archive snapshot fields but omits `admitted_by` and the admission vectors.
Run history is retained separately and may prune, so the archive cannot safely rely on an old run blob to reconstruct its own durable record.

**Required alignment:** add the admission provenance required by the archive contract and state whether the full entry gate vectors also remain with the archive or only with the picked episode.

### M7 — The engine-blind intervention claims to withhold a field the informed call never receives

`docs/trade-opportunities.md:311-313` defines engine-blind as withholding engine sub-scores, target sets, and the stand-in conviction.
The informed Step-5g contract explicitly excludes the stand-in because Step 5h cannot compute it until the model's milestone plan exists at `docs/trade-opportunities-workflow.md:529-536` and `logic-flow-docs/trade-opportunities-logic-flow.md:1011-1024`.
The intervention therefore cannot withhold that value relative to the informed call.
Execution mechanics are intentionally deferred, but the intervention's differential payload still has to be internally coherent and reconstructable.

**Required alignment:** remove stand-in conviction from the engine-blind withheld set or change the informed-call architecture and causal ordering explicitly.

### M8 — Data Sources collapses Quick and Deep Audit into one per-name subset

`docs/data-sources.md:697-706` says an ATO run spends “the same per-name subset” over selected names.
Quick Audit uses quote, estimates, cached dated bars, a filing-cadence rider, and conditional FINRA at `logic-flow-docs/trade-opportunities-logic-flow.md:1325-1354`.
Deep Audit uses the full Step-5 candidate surface and fresh web research at `logic-flow-docs/trade-opportunities-logic-flow.md:1356-1369`.
The unqualified sentence can direct an implementation either to over-fetch Quick Audit or under-fetch Deep Audit.

**Required alignment:** qualify the maintenance subset as Quick-Audit-only and point Deep Audit to the full per-candidate surface.

### L1 — Interface assigns the local suite's Schwab outage to Portfolio Step 2

`docs/interface.md:101-103` says a Schwab API outage surfaces when holdings are fetched at Step 2.
That is correct for Portfolio, but Trade Opportunities degrades a candidate's option-chain failure and fail-softs its closing Step-8 holdings cross-reference at `logic-flow-docs/trade-opportunities-logic-flow.md:418-425`, `logic-flow-docs/trade-opportunities-logic-flow.md:743-747`, and `logic-flow-docs/trade-opportunities-logic-flow.md:1244-1252`.
`docs/schwab-integration.md:76-81` already states the correct cross-job split.

**Required alignment:** qualify the Step-2 statement as Portfolio-only and summarize the two Trade Opportunities Schwab failure paths separately.

### L2 — Step 8 is typed as network-free even though it fetches Schwab holdings

The workflow defines `Computed` as no external network and requires networked model-free work to say `Computed + API retrieval` at `docs/trade-opportunities-workflow.md:28-30`.
The workflow table and Step-8 section classify the holdings cross-reference as only Computed at `docs/trade-opportunities-workflow.md:67` and `docs/trade-opportunities-workflow.md:696-699`.
The same section then pulls holdings fresh from Schwab at `docs/trade-opportunities-workflow.md:701-702`.

**Required alignment:** type Step 8 as Schwab API retrieval plus deterministic computation.

### L3 — M&A cardinality does not distinguish a network request from a local match

`docs/data-sources.md:740-745` labels `mergers-acquisitions-latest` as discovery plus per-candidate, under a taxonomy where per-candidate means budget-scaling requests.
The workflow descriptions say each candidate's M&A involvement is matched against the already fetched market-wide feed at `docs/trade-opportunities-workflow.md:337-342` and `logic-flow-docs/trade-opportunities-logic-flow.md:735-739`.

**Required alignment:** label this as one discovery or run-level fetch plus a local per-candidate lookup if no second network call occurs.

### L4 — The pipeline misidentifies which discovery feeders are research-active

`docs/trade-opportunities.md:190-204` says the first two of three feeders are research-active.
Its second feeder is explicitly deterministic bottom-up screening, while its third carried-watchlist feeder owns the bounded research refresh lane.
The logic flow assigns web work to Step 3b and selected Step-3c nodes at `logic-flow-docs/trade-opportunities-logic-flow.md:490-515` and `logic-flow-docs/trade-opportunities-logic-flow.md:597-628`.

**Required alignment:** say Step 3b is always research-active and Step 3c is conditionally research-active for the selected research-class refresh lane.

### L5 — The human glossary overstates what is outcome-scored

The glossary says the model arm is scored against the engine baseline at `logic-flow-docs/trade-opportunities-logic-flow.md:121-124`.
The detailed safety rule correctly narrows scoring to the two arms' entry-vintage target bands and leaves conviction, sub-scores, tier, horizon, runway, and other authored reads unscored at `logic-flow-docs/trade-opportunities-logic-flow.md:1388-1395`.
The technical workflow agrees at `docs/trade-opportunities-workflow.md:587-590` and `docs/trade-opportunities-workflow.md:678-679`.

**Required alignment:** say the two arms' target bands are scored identically against realized outcomes and compared head-to-head; the remaining model-arm fields are persisted but unscored.

### L6 — “Rejected story stock” blurs a persisted decision class

The logic-flow glossary calls a name without a measurable leading metric a rejected story stock at `logic-flow-docs/trade-opportunities-logic-flow.md:92-95`.
Its own evidence-floor contract calls that outcome `insufficient-evidence` at `logic-flow-docs/trade-opportunities-logic-flow.md:151-153`.
The workflow persists it as an abstention distinct from `gate-reject` at `docs/trade-opportunities-workflow.md:585-599`.

**Required alignment:** use the precise abstention vocabulary wherever a persisted outcome class is implied.

## Shared specification gaps that are not cross-document contradictions

The following rules are broadly stated the same way across the documents, but an implementer would still have to invent material behavior.
They should be resolved before their owning slices are marked implementation-ready.

### G1 — Diversity allocation is not executable for small or infeasible slates

`logic-flow-docs/trade-opportunities-logic-flow.md:650-657` and `docs/trade-opportunities.md:603-612` define simultaneous percentage floors and ceilings but no integer rounding rule, no relaxation order when the set is infeasible, and no counting rule for a name carrying multiple feeder or theme tags.
A one-slot or two-slot new-name remainder already makes several ceilings impossible if read literally.
The `signal strength × house-view fit` ranking and equal cap-band × sector allocation also lack a deterministic score and tie-break.

### G2 — Turn-away decision-class precedence is missing

The shadow ledger promises one typed episode per turn-away, but a debut may be below the evidence floor, fail both entry gates, and trip a hard trigger in the same pass.
The documents separately prescribe `insufficient-evidence`, ordinary `gate-reject`, and hard-trigger gate-reject-class episodes at `logic-flow-docs/trade-opportunities-logic-flow.md:1090-1131` without an ordered primary-class rule.
The precedence matters because scorecard populations are intentionally never pooled.

### G3 — Risk-tier null semantics and two predicates are undefined

The any-high, all-low, else-medium rule appears at `logic-flow-docs/trade-opportunities-logic-flow.md:1072-1075` and `docs/trade-opportunities.md:603-606`.
Neither document says how missing profitability, leverage, volatility, drawdown, or liquidity behaves.
Neither gives an exact predicate for `illiquid` or `high event exposure`.
Because the engine tier scales both arms' entry hurdle, a silent Medium fallback would change admissions materially.

### G4 — The target-band outcome scorer is named but not defined

The documents call for one interval scorer and say it tests whether a band contains the realized outcome at `docs/trade-opportunities.md:505` and `docs/trade-opportunities.md:519-526`.
They do not define the score, width penalty, role of the base target, treatment of the fixed twelve-month band at the 1-, 3-, and 6-month windows, aggregation, or the head-to-head comparison statistic.
Binary containment alone rewards arbitrarily wide bands, so “same scorer” is not yet a codeable calibration contract.

### G5 — Rotation ordering does not guarantee the stated service level

The rotation slice prioritizes warnings, catalyst proximity, and gate proximity before staleness at `logic-flow-docs/trade-opportunities-logic-flow.md:650-659`.
The same contract says overdue overflow remains in that priority order while also promising it is drained stalest-first and that no live opportunity can age indefinitely.
Without overdue precedence, aging, or a reservation rule, a steady arrival of higher-priority names can starve an old opportunity.

## Explicitly declared deterministic gaps

The logic flow openly identifies eight definitions as not yet drafted.
These are aligned omissions rather than contradictions, but development cannot implement the affected financial rules without a product or calibration decision.

- Discovery price, volume, and market-cap floor values — `logic-flow-docs/trade-opportunities-logic-flow.md:506`.
- SUE surprise-scaling window — `logic-flow-docs/trade-opportunities-logic-flow.md:509`.
- Commodity-price-turn rule and thresholds — `logic-flow-docs/trade-opportunities-logic-flow.md:511`.
- Archetype-classification feature formulas and cut-points — `logic-flow-docs/trade-opportunities-logic-flow.md:704`.
- Sector-adjusted factor-band values — `logic-flow-docs/trade-opportunities-logic-flow.md:784`.
- Exact archetype weight vectors — `logic-flow-docs/trade-opportunities-logic-flow.md:809`.
- Cost-of-capital and R&D-capitalization conventions — `logic-flow-docs/trade-opportunities-logic-flow.md:822`.
- Tradability-band boundaries — `logic-flow-docs/trade-opportunities-logic-flow.md:874`.

## Deliberate deferrals and acceptable logic-flow omissions

The blind-diagnostic execution design is intentionally deferred at `docs/trade-opportunities.md:321-325` and `docs/trade-opportunities-workflow.md:547-549`.
That is acceptable for scheduling, eligibility timing, timeouts, budget treatment, execution-state vocabulary, storage schema, and arbitration, provided the retained intervention definition is corrected under M7.
The retained boundaries otherwise align: engine-blind and realized-move-blind remain separate interventions, their output is diagnostic-only and reconstructable, a diagnostic-local failure is fail-soft, and global cancellation still governs the run.

The technical documents may enumerate more exact endpoints, cache behavior, source provenance, label-time dividend pulls, model modes, or persistence fields than the human-readable logic flow.
Those are acceptable omissions where the shorter flow preserves ownership, ordering, and behavior.
The logic flow does not need to duplicate the full report schema it consumes at Step 2; the required report sections and summary metadata exist consistently in `report-structure.md`, `report-workflow.md`, and `storage.md`.
The future representative-universe graduation path for factor distributions remains explicitly unscheduled in the technical design and does not need to appear in the logic flow.
Scorecard display remains explicitly deferred, so its absence from Step 10 is not a contradiction.
Tier, horizon, and runway divergence is currently per-pick and unscored, while only band and conviction divergence rates are pooled; a future pooled placement-divergence rate is an optional policy choice, not a present conflict.

## Areas confirmed aligned

- Both document sets mark Trade Opportunities as designed and not built.
- DTO's main Step-1 through Step-10 ordering and ATO's Quick and Deep reuse agree.
- The discovery feeders, coverage rotation, graph-blind outside-view route, carried watchlist, and no-bulk-pre-score intent agree apart from H1, M1, and L4.
- Per-topic depth, fetch and wall-clock ceilings, topic isolation, evidence-ledger handling, and single-versus-hierarchical distillation agree apart from the Step-3b card-formation seam in H1.
- The model roster and modes agree: the 122B reasoner handles thinking research and scoring, non-thinking distillation is permitted under the pinned runtime rule, the 35B model is optional for eligible distillation, and the fixed 4B model embeds.
- The two-arm contract, model-authored placement, engine-derived shared gate legs, either-arm admission, and arm-independent evidence and hard-forensic floors agree.
- The model's tier × horizon places the card, while engine tier, horizon, and runway persist as the disclosed baseline and alone supply the shared gate legs.
- Quick Audit is engine-only, may not rewrite, re-place, archive, clear warnings, or checkpoint, and may run with the configured daemon down.
- Deep Audit reuses the Step-5 loop and may archive only after a qualifying deep verdict.
- Resume consistently reopens the interrupted run id, pins upstream context and the candidate slate for the resume window, discards checkpoints on a new Discover run, and retains only the document cache across runs.
- On-demand scheduling, the single global run slot, cooperative cancellation, fail-soft discovery and per-candidate web research, hard rate-anchor failure, persistence failure, and diagnostic-local fail-soft behavior agree at the analytical level.
- Local-model memory stays partitioned by job and lifecycle; fresh archive re-entry is intended to retrieve no old-lifecycle record.
- The report, thesis-continuity, and Portfolio documents need not restate the Trade Opportunities workflow where their shared upstream or engine contracts are otherwise accurate.

## Development-readiness sequence

The documents should not be marked implementation-ready until the H and M findings have explicit rulings and all live consumers have been swept.
A safe resolution order is:

1. Settle job identity, the Step-3b call graph, Step-5 status transitions, and the six-store ownership model.
2. Settle lifecycle invariants: archive re-entry, immutable sector stamps, warning clearing, hard-trigger outcomes, turn-away precedence, and Quick-Audit label ownership.
3. Settle source cardinalities and the exact Step-5g payload.
4. Settle the five shared specification gaps and the eight explicitly undrafted deterministic rules before their implementation slices begin.
5. Sweep the logic flow, both owning Trade Opportunities documents, and every shared consumer named in this report in one correction round.

## Verification performed

The audit was a static documentation comparison only.
No Rust, Vue, tests, builds, clippy checks, or runtime probes were involved or appropriate.
The final integrity checks for this artifact are path and line-reference review, sentence-per-line review, a no-index whitespace check for the new untracked file, and confirmation that the worktree change is limited to this new verification file.

## Resolution — 2026-08-19

Every finding above was independently re-verified against the cited passages, with verbatim quotes and a search for refuting passages, before any correction was written; the corrections below then landed in one sweep across the logic flow, both owning Trade Opportunities documents, and every shared consumer this report names.
This section records the verification verdicts, the seven user rulings the resolution required, and the disposition of every finding.

### Verification verdicts and pushbacks

Twenty-seven of the twenty-eight items hold in some form; one is refuted, and several projected consequences were overstated.

- **H6 is refuted as a contradiction.**
  There are three sector-identity carriers with three deliberate freshness rules — the live matrix record refreshed at each deep pass, the picked episode frozen at entry, the archive frozen at departure — all specified consistently at `trade-opportunities.md` §Outcome learning, the exact section `storage.md` cites.
  The one real defect was terminological: `storage.md` called the deliberately refreshed live-record field "entry-stamped", and the fix is a gloss distinguishing it from the episode's frozen entry stamp, not a contract change.
- **H1's bottom line holds for the workflow's call block read alone but not for the corpus** — the logic flow and `web-research.md` already answered who authors the cards; the fix is restructuring the one under-specified block.
- **H4's projected identity reset is refuted** — lifecycle ids and `became_opportunity_at` are app-assigned and status is never the transition's control; the genuine gap was the undefined effective status and branch selection.
- **H5's projected consequences were overstated** — the two canonical homes agree on six stores and the workflow elsewhere states the episodes' and ledger's retention independence; the one substantive residue was the discovery-coverage ledger's absence from `data-portability.md`.
- **H9 was narrower than claimed** — tracker placement, page ownership, and run-slot behavior were already resolved by stated general rules; the genuine ruling was the `job_type` identity and the footer-stamp policy.
- **M1 was one loose noun, not two contradictory placements** — the cited sentence self-disambiguates in its next line; "longlist" is nonetheless a defined funnel term, so the wording was fixed.
- **M3's shadow half does not exist** — ATO selects only matrix names, and shadow episodes are by definition not in the matrix; the picked half resolved editorially, since labels are engine-computed and the ATO-refreshed-label case was already handled twice in the embedding dedup rule.
- **L6's suspected class conflation is refuted in the auditor's favor** — there is no separate leading-metric hard gate (`trade-opportunities.md` §Evidence floor states the leading-metric gate *is* part of the floor), so the glossary's "rejected" indeed implied the wrong persisted class and now names the floor abstention.
- **G4's scorer exists** — the width-penalized interval score is defined at `portfolio-analysis.md` §Outcome learning and shipped in `outcome.rs`; the genuine TO-side gap was the missing pointer and the unstated matching-window rule for the fixed twelve-month bands.
- **H2 was slightly stronger than claimed** — the logic flow's own Step-9 store summary repeated the impossible all-classes digest requirement alongside the correctly scoped Step-5h rule.

### Rulings

Seven rulings landed via the option dialog on 2026-08-19:

1. **Step-5g status schema is origin-constrained** (H4): a debut's call carries no status choice — the app stamps `new` — and a carried name's schema offers only `still-valid` / `invalidated`, so grammar-constrained output makes an origin-incompatible value structurally impossible.
2. **Turn-away classing precedence is hard trigger, then evidence floor, then ordinary gate-reject, first match wins** (G2); the losing conditions ride the episode's recorded content, never a second episode.
3. **The rotation keeps warnings-first but reserves at least one slot per slice for the overdue backlog, drained stalest-first** (G5), making the max-age liveness promise structural rather than best-effort.
4. **DTO and ATO share one `trade_opportunities` job identity** (H9): each run record carries its mode (`discover` / `audit-quick` / `audit-deep`), and history, retention, and the section footer read one mode-labeled pool.
5. **Every Trade Opportunities run stamps the footer, mode-labeled** (H9 follow-on) — a Quick Audit reads as the latest run without masquerading as discovery freshness.
6. **G1's allocation mechanics and G3's two predicates are marked inline as not yet drafted**, joining the eight declared deterministic gaps for the implementation plan's sweep; only the ranking tie-break (resolve by ticker) drafted now.
7. **Five defaults confirmed**: the Step-5g digest binds only post-5g turn-away classes (H2); the filing-cadence rider is FMP-only with SEC XBRL per-candidate (H8); the archive row carries `admitted_by` alone, the gate vectors living on the picked episode (M6); Trade Opportunities adopts Portfolio's missing-input tier rule — a missing leg cannot trigger, wholesale-missing reads Medium with a logged tier-input gap (G3); and the fixed twelve-month bands score at the matured 12-month window only, the 1 / 3 / 6-month labels serving the cohort reads, with `portfolio-analysis.md` §Outcome learning named the interval scorer's single home (G4).

### Dispositions

Every H, M, and L finding and every G gap is resolved; the corrections are docs-only (the job is unbuilt).

- **H1** — the workflow's Step-3b call block split into two: a per-topic *Route research* call (tool-using, findings only, the route's source-strategy rubric added to its inputs) and a per-route *Card formation* call (non-tooling, the accumulated topic findings + evidence ledger whole, reduce form for heavy routes).
- **H2** — the digest scoped to post-5g classes in `trade-opportunities.md`, `storage.md`, and the logic flow's Step-9 summary; pre-5g deferrals and retirements carry the watchlist node's persisted context instead.
- **H3** — the archive re-entry parenthetical re-scoped in `trade-opportunities.md`: a re-entry is a debut carrying no since-flagged read at its first Step 5g.
- **H4** — the origin-constrained enum written into the workflow's 5g Returns, the logic flow's 5g Returns, `trade-opportunities.md` §The opportunity, and Step 7's status vocabulary.
- **H5** — the workflow's Step-9 audit sentence now names the shadow scorecard's derived reads with the ledger an independent store, its store enumeration extended to all six, `configuration.md`'s parenthetical scoped, and the discovery-coverage ledger added to `data-portability.md` §Build-order placement.
- **H6** — a gloss only: `storage.md`'s live-record field renamed "stamped sector identity" with the freeze-at-last-seen rule and its distinction from the episode's frozen entry stamp stated.
- **H7** — `portfolio-analysis.md`'s rationale clause now states both Trade Opportunities hard outcomes before the Portfolio translation.
- **H8** — "SEC XBRL" removed from the rider at workflow §Step 3c and logic flow Step 3c, matching `data-sources.md` and the logic flow's own Step 7.
- **H9** — the identity ruling written into both TO docs, the logic flow's terms, `scheduling.md` §Job Status Visibility (with the mode-labeled stamp rule), `interface.md` (footer mapping and sidebar swap), `run-tracking.md` (page replacement), `configuration.md`, and `data-sources.md`'s job list.
- **M1** — "screener-narrowed longlist" replaced with the budget-narrowed candidate slate at all three `data-sources.md` sites.
- **M2** — "the next floor-clearing deep pass" written through the four broad-form sites (logic-flow glossary, `trade-opportunities.md` twice, workflow §Step 7, `storage.md`).
- **M3** — a mode-neutral label sentence added to the workflow's ATO close and the logic flow's Quick Audit persist leg: both audit modes refresh and record the engine-computable labels for the names they touch, `research`-class state alone held for a deep pass.
- **M4** — `continuity_weight` framing, the `validated_leading_indicator`, and the `technology_read` added to the workflow's exact 5g prompt block.
- **M5** — the `trade-opportunities.md` roster entry now states both arms with the model's bands the card's headline and the engine set the disclosed baseline.
- **M6** — admission provenance added to `storage.md`'s archive snapshot (as `admitted_by` alone), the same scoping noted at `trade-opportunities.md` §Archived opportunities.
- **M7** — the stand-in conviction removed from the engine-blind withhold list, with the causal reason stated in place.
- **M8** — `data-sources.md`'s ATO sentence qualified by fork: the maintenance subset is Quick-Audit-only, a Deep Audit spends the full per-candidate surface.
- **L1** — `interface.md`'s Schwab clause rewritten to the per-job split with a pointer to `schwab-integration.md` §Failure posture.
- **L2** — Step 8 retyped Computed + API retrieval in the workflow's table and section header.
- **L3** — the M&A row re-tagged on the FINRA row's idiom: fetched once per run plus a per-candidate local lookup.
- **L4** — the feeder sentence corrected: the hypothesis lane always research-active, the watchlist conditionally so, the structured screens deterministic.
- **L5** — the glossary's scoring clause narrowed to the target bands, the other authored reads recorded unscored.
- **L6** — the glossary's "rejected" now names the `insufficient-evidence` floor abstention.
- **G1** — the tie-break drafted (resolve by ticker); the rounding rule, infeasibility relaxation order, multi-tag counting rule, and ranking formula marked not yet drafted at `trade-opportunities.md` §Starting parameters, workflow §Step 4, and the logic flow's Step 4.
- **G2** — the precedence rule written into workflow §Step 5h and the logic flow's shadow-ledger bullet, with a pointer line in `trade-opportunities.md` §Outcome learning.
- **G3** — the missing-input rule adopted at the tier rule's canonical row and the logic flow's Step 5h; the `illiquid` and `high event-exposure` predicates marked not yet drafted in both.
- **G4** — the scorer pointer and the 12-month matching-window rule written into `trade-opportunities.md` §Outcome learning (twice, replacing the loose containment gloss), workflow §Step 7, and the logic flow's outcome-learning section.
- **G5** — the reserved-overdue-sub-slot rule written through workflow §Step 4, the logic flow's Step 4, `trade-opportunities.md` (§The two jobs, §Archived opportunities twice), and `configuration.md`.

The eight explicitly declared deterministic gaps stay parked unchanged — the implementation plan's to sweep, per the standing decision — now joined by G1's mechanics and G3's predicates under ruling 6.
The optional pooled placement-divergence rate remains an open question, deliberately not ruled here.
`BUILD.md` and `INDEX.md` absorption of the new contracts (the job identity and mode, the turn-away precedence, the reserved overdue sub-slot, the origin-constrained status) is left to the user per the Metis write discipline.

### Review round 1 (Codex, 2026-08-19)

Codex reviewed the sweep and withheld approval on five findings plus two list-indentation slips; all seven were verified against the files, all held, and all are fixed.

- **Card-formation seed lineage** (medium) — the Step-3b restructure's card-formation call required `seeded_by` validated against the route's fed seed-ID set without receiving the seed records (a gap the split itself created — the original monolithic block had carried them); the route's fed seed records with their stable ids now ride the card-formation call directly in both its ordinary and reduce forms, in the workflow and the logic flow.
- **The interval scorer was still not a codeable documentation contract** (medium) — the pointer round left the equation, the coverage→penalty mapping, the aggregation, and the head-to-head statistic only in `outcome.rs`; the as-built contract is now written at the single home, `portfolio-analysis.md` §Outcome learning: the Winkler score for a central `(1 − α)` interval — width plus `2 ⁄ α` times any exceedance, `α = 1 −` nominal coverage (80% → `α = 0.2`), lower better, in return space through the anchor-close bridge — cohort read the arithmetic mean per matching window per arm, head-to-head each arm's mean over the paired events compared directly.
- **`storage.md`'s run-audit sentence re-universalized the digest and gate vectors** (medium) — the shadow-episode clause now defers to the class-specific schema (digest on post-5g classes, gate vectors on a gate-reject).
- **The logic flow's Quick Audit said "No Schwab data" while its closing Step 8 pulls holdings** (low) — qualified to the analytical pass, the fail-soft display-only Step-8 pull named, matching the workflow.
- **The logic flow's Step-9 run record still called the live sector field "entry-stamped"** (low) — renamed to the stamped sector identity with the refresh rule, the last H6 residue.
- Two list-indentation slips in newly added prose (the Step-5h precedence sentence and the ATO continuity paragraph) re-indented to their list levels.

### Review round 2 (Codex, 2026-08-19)

Codex confirmed every round-1 fix and found one low-severity residue: the logic flow's ATO gate bullet still said Quick Audit "reads no Schwab data" under a different phrasing than the round-1 grep matched.
The parenthetical now carries the same analytical-pass qualification as the data-retrieved bullet, naming the fail-soft, display-only Step-8 cross-reference as the one Schwab touch.
