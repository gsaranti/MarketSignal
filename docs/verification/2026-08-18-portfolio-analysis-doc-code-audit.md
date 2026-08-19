# Portfolio Analysis documentation and code audit (2026-08-18)

## Verdict

The built Portfolio Analysis spine is substantially coherent, but the documentation and implementation are not yet fully aligned.
The audit found **2 high-severity contract conflicts, 10 medium-severity mismatches, 8 low-severity residues, and 1 additional research-loop decision that must be pinned before implementation**.
The two high-severity findings are both in the human-readable logic flow: its stock evidence floor contradicts the listing guard, and its research-persistence statement contradicts the technical storage contract.
The most important current-code mismatches are the late acquisition of the global run slot, a run-date split on sector-P/E retrieval, inaccurate audit provenance, and broad prior-state read failures being treated as an empty first-run state.

No existing documentation or code was changed by this audit.
This file is the only intended worktree addition.

## Scope and authority

The primary comparison was:

- `logic-flow-docs/portfolio-analysis-logic-flow.md`, the human-readable end-to-end explanation;
- `docs/portfolio-analysis.md` and `docs/portfolio-workflow.md`, the canonical Portfolio technical contracts;
- the Portfolio-relevant sections of `docs/storage.md`, `docs/interface.md`, `docs/configuration.md`, `docs/data-sources.md`, `docs/web-research.md`, `docs/local-models.md`, `docs/local-model-operations.md`, `docs/schwab-integration.md`, `docs/scheduling.md`, `docs/run-tracking.md`, and `docs/data-portability.md`;
- the production Tauri command path, `src-tauri/src/portfolio/**`, shared adapters and persistence, `src/App.vue`, `src/components/PortfolioView.vue`, `src/types.ts`, and their Rust/frontend tests.

Dated files in `docs/verification/` were treated as historical evidence and rulings, not current behavior contracts.
The undated `docs/verification/big-run-watch-set.md` was treated as a living forward checklist because it explicitly describes itself that way.
The technical corpus is intentionally forward-looking, so an absent implementation was not classified as a defect where the built-versus-designed boundary is clear.

## As-built versus designed map

| Area | As-built behavior | Designed or deferred behavior |
|---|---|---|
| Step 1 | Local-model/FMP/FRED/Schwab gate and one global run slot | No separate research-backend gate; the ordering mismatch is finding M1 |
| Step 2 | Fresh Schwab pull, account-row normalization, run snapshot | Manual-import source and resumable pinned snapshot |
| Steps 3–4 | Asset eligibility, listing guard, deterministic prior-run holdings diff | No material deferred core |
| Step 5 | Freshness-gated house view, fixed investor preset, run-level FRED anchors | Commodity/CFTC/CBOE/benchmark context extensions |
| Step 6a | FMP/SEC stock packet, reduced fund packet, deep EOD, option-activity signal, prior verdict/ledger, sector-P/E | Same-underlying held-option overlay, news seed, richer feeds, semantic recall |
| Step 6b | Deterministic stock/fund engine, evidence floor, grades, targets, hurdle, role-risk branch, statement/carry pre-profit overlay | Narrative/forensic/implied-expectations/input-delta depth |
| Step 6c | Fixed no-network `Web research deferred` artifact for the priced branch; role-risk bypasses the stage | Bounded per-topic tool loop, evidence ledger, document cache, per-topic seed-and-merge |
| Step 6d | One unconstrained non-thinking condense of the priced stub note; no role-risk distillation | Schema-bound single/hierarchical consolidation and reconciled per-topic seed layer |
| Step 6e | No separate recomputation; 6b output passes through | Forward-assumption target/hurdle refinement and research-observation overlay refinement |
| Steps 6f–6g | Priced or role-risk interpretation, separate per-holding action call, ledger validation, engine stamps, outside-set annotation | What-changed attribution validator, research-backed qualitative validation, checkpoint/resume |
| Step 7 onward | Deterministic roll-up, outcome lifecycle, atomic run/episode persistence, best-effort matured-learning embedding | Per-holding verdict embeddings and partial-run resume state |
| UI and quick paths | Read-only Portfolio page/history, selection/carry badges, pull-only view, engine-only Quick check | Research-backend notice/modal and other research-dependent surfaces |

The logic-flow document splits roll-up/outcomes, persistence, and display into Steps 7, 8, and 9.
The workflow document combines outcome/persistence as Step 7 and calls display Step 8.
That is a granularity difference, not a behavioral contradiction, but cross-document references should continue to use section names rather than bare step numbers.

## High-severity contract conflicts

### H1 — The designed research record is both persisted and transient

The logic flow says the future research-reuse decisions and assumptions will persist, but that “distilled research itself is a transient prompt input, not persisted” (`logic-flow-docs/portfolio-analysis-logic-flow.md:1408-1409`).
The technical contract says the full run audit persists distilled findings (`docs/portfolio-analysis.md:698-699`; `docs/portfolio-workflow.md:379-381`; `docs/storage.md:212-215,232-234`).
Both document families separately agree that the globally reconciled per-topic seed layer is persisted for the next run (`docs/portfolio-workflow.md:248-252`; logic flow `1105-1109`).

These statements leave two distinct research artifacts without one storage ruling:

- the combined cross-topic object consumed by interpretation and the audit; and
- the reconciled per-topic objects used as the next run's seeds.

Before Step 6c/6d is implemented, decide whether the combined object is stored in the run audit or remains transient while only the per-topic layer persists.
The decision changes the durable schema, portability surface, audit reconstruction, and pruning ownership, so it should not be left to the implementation slice.

### H2 — The human evidence floor contradicts the listing guard and the live target floor

The logic flow correctly says an unreadable, missing, or too-sparse profile/Schwab identity comparison proceeds with a degraded input (`logic-flow-docs/portfolio-analysis-logic-flow.md:455-461`).
Its later evidence-floor summary nevertheless says a stock requires “Matching issuer identity” (`:857-862`).
The technical contract and code make only a resolved conflict terminal; nothing trustworthy to compare is `unverified` and continues degraded (`docs/portfolio-analysis.md:101-107,400-404`; `pipeline.rs:208-215,333-390`).

The same logic-flow bullet says a usable target driver is required only “once the v2 function is active” (`:862`).
The v2 function is already active, and `no-admissible-driver` is a current floor exit (`docs/portfolio-analysis.md:103-105,405-407,425-426`; `engine.rs`).

As written, the human guide can lead a developer to mass-abstain unverified identities and to defer a live evidence-floor rule.
The intended current rule is: current price + statements + at least two real sub-scores + an admissible target driver, with only a positively identified issuer conflict adding an identity-floor exit.

## Medium-severity mismatches

### M1 — A concurrent attempt can perform live setup work before acquiring the global run slot

Step 1 says the global slot is a start precondition and that no investment-data API runs there (`docs/portfolio-workflow.md:54-68`; logic flow `245-262`).
Production probes Ollama, constructs the adapters, and calls `load_cik_resolver` before entering `run_portfolio_job` (`src-tauri/src/lib.rs:744-842`).
With an absent or stale CIK cache, that resolver performs a live SEC request and can rewrite the cache (`src-tauri/src/sec.rs:138-173`).
Only then does `run_portfolio_job` call `guard.try_begin` (`src-tauri/src/portfolio/job.rs:403-425`).

The analytical loop remains serialized, but an attempt that ultimately records `Skipped` because another job owns the slot can already have probed Ollama and contacted SEC.
Either acquire the slot before this setup or narrow the docs so they no longer promise that the global-slot refusal precedes external setup work.

### M2 — The sector-P/E snapshot can escape the run's pinned ET session

The job mints one `created_at` and one ET session date before dated decisions (`src-tauri/src/portfolio/job.rs:549-565`).
The docs say the sector-P/E snapshot begins from that run session (`docs/portfolio-workflow.md:153-154`; `docs/data-sources.md:643-644`).
`LiveCompanyData::sector_pe_snapshot`, however, calls `Utc::now()` when the first fund reaches the fetch (`job.rs:233-255`), while the fund context is later labeled with the earlier pinned `today` (`job.rs:820-862`).

A long run that crosses ET midnight before its first fund can therefore use next-session sector data inside a prior-session run.
The snapshot date needs to be injected from the run clock, as the fund context's `as_of` already is, or the documentation must abandon the shared-time-basis guarantee.

### M3 — The persisted audit can name models, metrics, and sources that do not match the work actually performed

`HoldingAudit` promises the computed metrics, sources consulted, and model IDs used (`src-tauri/src/portfolio/mod.rs:1279-1292`; `docs/storage.md:212-234`).
The current construction path does not consistently meet that promise:

- every audit row receives the analyst's configured reasoner/fast model list, including non-gradeable, zero/net-short, and terminal-listing paths that make no model call (`pipeline.rs:248-263,285-390,3130-3135`);
- role-risk uses the reasoner but can claim a distinct fast model, and its computed fund metrics are discarded in favor of `ComputedMetrics::default()` (`pipeline.rs:424-506`);
- FRED anchors influence priced verdicts but never appear in the source labels;
- the profile lookup drives listing identity, issuer name, and outcome sector identity but is normally absent from the source list;
- SEC is labeled only when it returned nonempty facts, not whenever it was consulted (`dossier.rs:390-410`).

This is more than a missing rich-research audit: the as-built labels can be factually inaccurate.
Audit provenance should be collected from completed stages/consulted adapters, and branch-specific computed metrics should persist where the contract says they do.

### M4 — Arbitrary prior-state read failures are silently treated as first-run emptiness

The job loads both `latest_run` and `latest_quick_check` with `.ok().flatten()` (`src-tauri/src/portfolio/job.rs:530-547`).
`latest_run` already skips corrupt blobs explicitly and loudly inside the store, so remaining `Err` values include SQL/query failures (`store.rs:363-395`).
Quick-state JSON and SQL failures likewise surface as `Err` from storage before being swallowed (`store.rs:277-302`).

The documented corrupt-row policy does not authorize treating every persistence failure as “no prior state” (`docs/storage.md:158-169`; `docs/portfolio-analysis.md:731-743`).
The fallback can erase continuity for the run in progress, mark every holding new, discard quick-check chaining, and convert a requested selective run to full-book behavior.
Only the named corrupt-row case should fail soft; store-level errors should be surfaced or explicitly governed by a documented recovery rule.

### M5 — Selective analysis has an undocumented full-book fallback when there is no readable baseline

The public contract says a nonempty selection analyzes strictly those holdings (`docs/portfolio-analysis.md:15-19`; `docs/portfolio-workflow.md:15,136-140`; logic flow `419-438`).
`SelectiveRun` and the work-list implementation instead define no prior run to carry from as full-book (`src-tauri/src/portfolio/job.rs:350-360,609-663`).

The ordinary UI cannot create a selection before it has a rendered run, but the command accepts the shape, and M4 makes the exception reachable after a prior-state read error.
Pin one rule: reject a selective request without a readable baseline, honor only the selected intersection and leave the rest not analyzed, or document that the request deliberately expands to a full run.

### M6 — The action rationale's exact response contract is prompt-only

The docs and Rust type call the result schema-constrained, exactly keyed, nonempty, and one sentence (`docs/portfolio-analysis.md:318-331`; logic flow `1258-1289`; `src-tauri/src/portfolio/mod.rs:1816-1852`; `pipeline.rs:1969-2005`).
The JSON schema requires the two properties but permits any string and does not set `additionalProperties: false` (`mod.rs:1850-1868`).
Serde then deserializes and persists the response without validating nonempty/one-sentence semantics (`pipeline.rs:3106-3127,496-505,638-651`).

An empty, multiline, or multi-sentence rationale can therefore complete a live run, and unknown keys are silently ignored.
Either describe one-sentence rationale as a prompt preference or add the application validation implied by the current contract.

### M7 — The exact recent-report house-view payload differs between the workflow and production

The workflow says recent report summaries contribute `thesis_stance`, `forward_outlook_themes`, and `key_risks` (`docs/portfolio-workflow.md:116-124`).
The logic flow says the model receives date, thesis stance, and risk posture (`logic-flow-docs/portfolio-analysis-logic-flow.md:1205-1208`).
Production matches the logic flow, rendering `created_at`, `thesis_stance`, and `risk_posture` (`src-tauri/src/portfolio/pipeline.rs:1907-1915`).

This is an exact model-input contract, not an allowed difference in explanatory depth.
The workflow should name the fields actually rendered, or the prompt should be changed to match the intended technical packet.

### M8 — The logic flow overstates the current persisted source/timestamp surface

Step 8 says its unmarked list is populated today, then includes “Sources and timestamps” (`logic-flow-docs/portfolio-analysis-logic-flow.md:1392-1409`).
The canonical storage section says the as-built Portfolio audit has source labels only, not URLs or retrieval timestamps, and that research-derived timestamps land later (`docs/storage.md:212-234`).

Split the logic-flow row into the current labels and the designed research provenance so the human guide does not claim a traceability surface that does not exist.
This is separate from M3, which finds that even the current labels are sometimes inaccurate.

### M9 — The pre-run research notice preserves the retired reuse-skips-the-loop model

The designed pre-run notice says a selective Portfolio run may “reuse all cached research” and that reuse is decided per holding (`docs/interface.md:161-166`).
The settled research contract is always-run seed-and-merge: every analyzed holding runs research and distillation; a fresh cache only seeds and merges (`docs/portfolio-analysis.md:252-253,544-560`; `docs/portfolio-workflow.md:141-142`; logic flow `911-925`).

Consent still belongs at job launch, but the rationale must not imply that a selected holding can skip live research because its cache is warm.

### M10 — The living big-run checklist still targets the removed construction job

`docs/verification/big-run-watch-set.md` defines itself as the forward-looking checklist for the next confirmation run (`:1-9`).
Its “Construction and the two-arm verdict” section still asks that run to measure construction, lean divergence, the construction prompt, and 7b sizing movement (`:77-88`).
Portfolio v9 removed the construction stage, whole-book lean reconciliation, and sizing; current actions are per-holding rungs and whole-book planning is deferred (`docs/portfolio-analysis.md:271-272,318-331`; `src-tauri/src/portfolio/job.rs:1087-1101`).

Unlike dated verification records, this file is operational guidance for a future run and is now unsafe to follow as written.

## Low-severity residues and UI boundary mismatches

### L1 — Malformed analysis vintages are stale in the backend but invisible as carried/stale in the UI

The backend treats an unparseable `analyzed_at` as over-age and applies stale-carry rules (`src-tauri/src/portfolio/job.rs:373-383,1000-1051`).
The UI's `carriedStamp` returns no badge when `etDayDiff` cannot parse the same value (`src/components/PortfolioView.vue:247-274`).
A rule-demoted add still gets its separate demotion badge, but a malformed carried hold/exit can lose the promised analysis-vintage warning.

### L2 — Not-analyzed placeholder cards do not participate in the technical sort-bar contract

The technical docs say the sort bar reorders the holding cards in place (`docs/interface.md:111`; `docs/portfolio-analysis.md:720-728`).
Production sorts only `run.verdicts`, then appends `notAnalyzed` placeholder cards after the sorted verdict stack (`src/components/PortfolioView.vue:505-521,1268-1274,2222-2252`).
The logic flow is more precise and says the bar is shown for and sorts verdicts (`logic-flow-docs/portfolio-analysis-logic-flow.md:1483-1485`).

### L3 — The Quick-check disabled explanation misclassifies a corrupt-only history

The page correctly distinguishes an unreadable-only history from a never-run store in its empty state (`src/App.vue:438-442`; `PortfolioView.vue:1001-1011`).
The disabled Quick-check title still says “Run an analysis first — there is no thesis ledger to check yet” whenever no readable latest run exists (`PortfolioView.vue:171-184`).
For a corrupt-only history, the actual condition is that no readable ledger is available, not that no analysis ever ran.

### L4 — One canonical verdict sentence still calls the action portfolio-aware

`docs/portfolio-analysis.md:278` calls action “the portfolio-aware decision over all three.”
The surrounding section and production correctly define the action as profile-aware but tunnel-vision, with no whole-book input (`:271-272,295-331`; `pipeline.rs:107-120,1969-2005`).

### L5 — The investor-profile cash paragraph retains retired concentration/risk-limit language

`docs/configuration.md:156-158` says concentration and risk limits still apply when cash is treated as always available.
Portfolio v9 removed concentration and buying-power gates from the action call; those are future planner concerns (`docs/portfolio-analysis.md:320-331`; `pipeline.rs:1976-1985`).
If the sentence is meant only for Trade Opportunities, it needs that scope; as shared profile guidance it currently reintroduces a Portfolio constraint that does not exist.

### L6 — The Quick-check introduction says only a full run tests the ledger immediately before saying Quick check does

`docs/portfolio-analysis.md:173-176` says a full run is “the only thing” that tests thesis-ledger conditions, then defines Quick check as the between-run condition test.
The intended counterfactual is that without Quick check only a full run would test them.

### L7 — Source comments and prompt terminology retain four obsolete claims

- `src-tauri/src/lib.rs:736-738` says a missing FMP key degrades to gaps, while `local_gate` blocks missing FMP/FRED (`local_model.rs:808-820`).
- `src-tauri/src/schwab.rs:6-12` says production still uses only the fixture and that live OAuth lands later, while `lib.rs:788-791` constructs the live source.
- `src-tauri/src/lib.rs:1541-1545` says the profile labels feed the removed Step-7b construction prompt.
- `src-tauri/src/portfolio/pipeline.rs:1415-1418` calls every below-US-exposure fund “ex-US,” while the canonical rule deliberately describes the measured below-70%-US exposure guard rather than asserting nationality (`docs/portfolio-analysis.md:94-98`).

These do not change control flow, but they are active source guidance and prompt text, so they can mislead development or the role-risk model.

### L8 — The history label “rated N” has no settled counting meaning

`docs/interface.md:73-76` promises a rated count on every readable Portfolio history row.
The logic flow records that the current number is priced/graded-only even though “rated” sounds as if analyzed role-risk holdings should count (`logic-flow-docs/portfolio-analysis-logic-flow.md:1473-1477`).
Settle the label or the aggregate before treating this UI contract as closed.

## Research-loop decision still open

`docs/web-research.md:134-136` adds one bounded disconfirming-fetch pass once a thesis forms.
The Portfolio loop independently defines one root plus at most two follow-up passes per topic, under a holding-level fetch/wall-clock budget (`logic-flow-docs/portfolio-analysis-logic-flow.md:975-1024`; `docs/portfolio-workflow.md:198-229`).
The corpus does not say whether the disconfirming pass is per topic or per holding, where it is scheduled, or whether it consumes one of the maximum three passes.
That topology and budget ownership should be resolved before the research orchestrator is implemented.

The role-risk research path does not require a separate ruling if the current status scoping is read carefully: as-built it bypasses 6c/6d, while the future contract gives every analyzed fund a fund agenda and describes role-risk output as pure consolidation with no priced typed fields (`docs/portfolio-workflow.md:133,213,235-236,265`).
That intended transition is easy to misread, so a one-sentence future-state qualifier would still be useful.

## Logic-flow completeness gaps that are not contradictions

The logic flow is allowed to omit technical depth, so the following were not counted as defects, but they are worth adding because each controls an operationally surprising boundary:

- it explains web retrieval failure as fail-soft but does not state that a required 6c–6f model-call failure hard-fails the run (`logic flow:1026-1036`; canonical rule at `docs/portfolio-analysis.md:739-748`);
- its Quick-check section omits the Schwab-connection and FMP/FRED-presence gate even though no Schwab request or model call occurs (`logic flow:1495-1588`; `docs/portfolio-workflow.md:414-425`).

The high-level present-tense voice in `docs/data-sources.md` and `docs/web-research.md` was also not counted by itself.
The documentation index says these are forward specifications, and the Portfolio endpoint table plus primary Portfolio docs mark the current wired subset.
The safer future editing pattern is nevertheless to keep the built/design qualifier next to any call-budget or privacy statement that a reader could reasonably interpret as current network behavior.

## Confirmed aligned behavior

The following high-risk seams matched across the logic flow, technical docs, production code, and tests:

- fresh Schwab holdings are normalized into one book row per symbol, while raw rows remain available for audit;
- the standalone Pull holdings snapshot is view-only and never becomes the analysis diff baseline;
- definitive empty/non-US listing resolution is terminal, while malformed/unavailable identity proceeds degraded;
- non-gradeable and nonpositive-net positions route before model work, and funds skip SEC;
- stock/fund engine outputs, evidence-floor abstention, target/hurdle calculations, and role-risk typing are deterministic;
- the current research stage does no web request, no cache/reuse, no evidence ledger, and no hierarchical distillation;
- interpretation is profile-blind, while the separate action call receives exactly one holding plus the fixed investor profile and no whole-book context;
- a selective run's readable-baseline path analyzes only the selected intersection, tail-sweeps only to preserve state/badges, carries prior verdicts, and leaves unselected no-prior holdings as placeholders;
- side-reversal, stale-add demotion, attention/evidence/degraded badges, and Quick-check warn-don't-rewrite behavior are preserved without force-including the tail;
- a FRED rate-anchor failure is hard before per-holding work, optional evidence failures are soft, and a per-holding model failure leaves no partial run;
- roll-up and outcomes are descriptive/deterministic, and run plus changed episodes persist in one transaction;
- checkpoint/resume, richer attribution, per-holding semantic recall/embeddings, full research/reuse, and target/observation refinement are documented as deferred rather than mistaken for current code;
- historical display is read-only, corrupt blobs remain listed unreadable, and the latest readable run supplies the normal UI/baseline path.

## Verification

Independent gates run on the audited tree:

- `cd src-tauri && cargo test` — **1,053 passed, 0 failed, 28 ignored** after rerunning with localhost binding permitted.
- The first sandboxed Rust attempt compiled but had **71 failures solely at mock/OAuth localhost binds** with `Operation not permitted`; the unrestricted rerun passed unchanged.
- `cd src-tauri && cargo clippy --all-targets --all-features` — passed, warning-free.
- `npm run build` — passed (`vue-tsc --noEmit` and Vite production build).
- `npm test` — **46 pure-module tests and 233 Vue component tests passed**.
- `git diff --check` — passed after this report was written.

No live Schwab/FMP/FRED/Ollama/SearXNG Portfolio run was performed.
Live-only request timing and model-output quality remain runtime confirmation items; the production paths were audited statically and the live smoke tests remain ignored by the normal suite.

## Dispositions (2026-08-18)

Every finding above was re-verified against the code and docs before it was acted on, and every one was addressed in the same session.
Nothing was refuted outright, but several findings were narrower or differently shaped than stated; the push-backs are recorded per row.
The four user rulings are marked **ruled**.

| # | Verdict on re-verification | Disposition |
|---|---|---|
| H1 | Confirmed, but not an open decision — the canonical docs already pin it (the run audit carries the combined distilled object; the per-topic seed layer persists as next-run seeds) | Logic flow Step 8 corrected to canonical; no schema ruling needed |
| H2 | Confirmed — the logic flow's own guard rules and Step 6b already stated the as-built rule | Logic flow evidence-floor bullets corrected (no resolved identity conflict; `no-admissible-driver` is a live exit) |
| M1 | Confirmed, and worse than stated: the SEC ticker-map fetch bails on a set cancel flag, which is only reset after the slot claim, so the first run after a cancelled run on a cold/stale cache silently gapped every EDGAR leg; the pre-slot request row was also dropped by the tracker | Code: CIK resolution is lazy inside the slot (`sec::LazyCikResolver`, both jobs); the local-only daemon probe stays the one pre-slot check; workflow Step 1 + logic flow record the ordering |
| M2 | Confirmed | Code: `sector_pe_snapshot` takes the run's pinned ET session date; recording-stub test |
| M3 | Confirmed, all five sub-claims (audit is persisted-only, never rendered) | Code: per-holding provenance collector — model ids recorded from calls actually made (no-model exit persists none, role-risk the reasoner alone), FRED / profile / SEC-consulted labels (an empty SEC read labels as empty), role-risk fund metrics persisted; storage.md pins the rule |
| M4 | Partial — fail-soft is deliberate and store-doc-acknowledged; the real gaps were the missing log line (report pipeline precedent logs) and `latest_quick_check` lacking the loud-skip, which also blanked the Portfolio page on a corrupt quick-check row | Code: logged fail-soft helper on both prior-state reads; quick-check read loud-skips like `latest_run`; storage.md + portfolio-analysis.md name the store-error leg |
| M5 | Confirmed divergence, low stakes — deliberate (`3603b01`), tested, UI-unreachable, never ruled | **Ruled**: document as-built — a selective request with no readable prior run runs the whole book (portfolio-analysis.md §Triggering canonical; workflow + logic flow pointers) |
| M6 | Confirmed as fact, overstated — the docs promise a one-line rationale, not "exactly keyed / nonempty", and no portfolio Ollama schema sets `additionalProperties:false` | Code: nonempty-rationale guard fails the holding (fail-hard, model-stage posture); one-sentence shape stays a prompt preference; docs say so |
| M7 | Confirmed — the fields exist on `ReportSummary` and reach the dossier, but the prompt renders date / stance / posture | Doc: workflow names the as-built payload |
| M8 | Confirmed | Logic flow Step 8 lists source labels; URLs / timestamps marked designed |
| M9 | Confirmed | interface.md pre-run notice restated to seed-and-merge |
| M10 | Confirmed, broader than the section named — the file carried no v9-revision marker | Watch set revised to the `portfolio-v9` shape (construction / lean / sizing / overlay-classifier watches removed; prompt-fit watch re-homed to the per-holding prompts; self-dated) |
| L1 | Confirmed, practically unreachable (only a non-RFC3339 stamp diverges) | Code: unparseable vintage renders the existing status tag "Stale · vintage unknown" |
| L2 | Confirmed — placeholders are `Position`s and every sort key is position-level | **Ruled**: code side — placeholders sort into the stack on the same keys; bar gated on total cards; docs updated |
| L3 | Confirmed (`unreadableHistory` prop already wired) | Code: corrupt-only history gets its own Quick-check disabled title |
| L4, L5, L6 | Confirmed (L6 partial — rhetorical) | Doc sentences corrected |
| L7 | Confirmed; extras found (`mod.rs` still cited a `lean` field and the 7b prompt; two test comments) | Comments and the role-risk system prompt corrected |
| L8 | Confirmed (already an open BUILD ruling) | **Ruled**: label renamed `graded N`; interface.md + logic flow updated |
| Research-loop pass | Confirmed unspecified | **Ruled**: once per holding after its topics, spent from the holding's budget, not counted against any topic's depth, fail-soft to a gap — canonical at portfolio-workflow.md §Step 6c; web-research.md and the logic flow point there |
| Role-risk research path | Partial — derivable | Workflow Step 6d one-sentence future-state qualifier |
| Completeness gaps | Both confirmed | Logic flow: 6c–6f hard-fail bullet; Quick-check gate bullet |

Post-fix gates: `cargo test` 1,033 passed / 0 failed / 28 ignored; `cargo clippy --all-targets --all-features` warning-free; `npm run build` passed; `npm test` 46 pure-module + 236 component tests passed; `git diff --check` clean.

### Codex review round 1

Five findings on the applied fixes; each was verified against the code before it was acted on.

- **Source labels recorded outcomes, not consultations** — confirmed on all three sub-points, and fixed: the SEC and option-chain legs now pass a shared `LegOutcome` (not run / empty / got) into `dossier::assemble`, a no-CIK ticker records its gap and no SEC label, a consulted chain that returned nothing labels as such, and every gradeable holding names the Schwab holdings snapshot.
- **The "exactly these keys … enforced by the decoder" contract sentence** — partial: the instruction stands, but the enforcement clause overclaimed, since no portfolio schema closes its object and no struct denies unknown fields; all three response-contract sentences now say the grammar enforces required keys and value shapes and that a key outside the set is dropped on decode. Schemas unchanged (earlier ruling).
- **Retired "lean" language in the shared pre-profit prompt section** — confirmed, and fixed: the section is stage-aware (`PromptStage`), so the interpretation prompt carries the conviction-ceiling guidance and states the engine's action-set narrowing as context only, while the action prompt carries the rung guidance and states the ceiling as context; "lean" is gone. Two Settings comments corrected.
- **Present-tense designed infrastructure in `web-research.md` / `configuration.md` / `data-sources.md`** — pushed back: `docs/README.md` opens with the Build-status banner declaring these docs forward design specs written in present tense, with build status single-homed in `BUILD.md`; per-sentence status markers would fork that home. Not changed.
- **The logic flow's `graded N` note overstated what sits beside the sidebar count** — confirmed; the note now says role-risk-only holdings are excluded from the sidebar number and appear only in the open run's key-figures strip.

Gates after the round: `cargo test` 1,035 passed / 0 failed / 28 ignored; clippy warning-free; `npm run build` passed; `npm test` 46 + 236 passed; `git diff --check` clean.

### Codex review round 2

One finding, confirmed on both sub-points and fixed: a guard-terminal stock now names the Schwab holdings snapshot beside the profile read that decided it, and a fund's FMP pull labels as `FMP fund financials (quote, EOD history, dividends)` — the surface `fetch_fund_financials` actually reads — rather than borrowing the stock statement / consensus label.
Codex withdrew its round-1 forward-spec finding on the `docs/README.md` Build-status banner.
Gates after the round: `cargo test` 1,035 passed / 0 failed / 28 ignored; clippy warning-free; frontend untouched this round (`npm run build` + `npm test` last passed at 46 + 236).
