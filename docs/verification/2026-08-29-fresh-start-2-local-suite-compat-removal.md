# Fresh-start 2 — local-suite compat removal

*2026-08-29.
A code + docs slice, not a point-in-time probe: the contracts it changed live in the docs cited below.
It extends the 2026-08-17 fresh-start legacy removal ([2026-08-17-fresh-start-legacy-removal.md](2026-08-17-fresh-start-legacy-removal.md)).*

## The ruling

The user ruled that the dev store is wiped before the big confirmation run and that, pre-release, **no persisted local-suite data survives a schema change** — data backward compatibility is not a requirement, and the layer written for it "overcomplicated the code for no reason".
This supersedes the 2026-08-17 record's kept class ("general `#[serde(default)]` forward-compat that is not construction-specific stays — ordinary schema evolution across binary versions"): on an app-written struct that attribute only ever reads rows an older dev binary wrote, so it goes too.
The rule for new slices is the I17 posture, never the I18 one: a new field is required and the producer always writes it; no serde default and no "a record persisted before the field reads as X" branch is written for local-suite data.

**The keep-vs-cut test is the writer of the JSON.**
App-written (a run, verdict, audit, ledger, episode, checkpoint header or row, quick-check state, research cache row) decodes strictly.
Model-written (the interpretation, the ledger drafts, the distillation and findings wire types, the typed side-channels decoded straight from the model's JSON) and provider-written (FMP, FRED, BLS, Schwab, Ollama, OpenAI responses) keep their lenience — that is schema tolerance for a JSON the app does not author, not data compat.
The 2026-08-31 attempt-4 Finding-4 closure narrows that historical rule for the research findings boundary: optional model-authored fields remain lenient, but keys the findings grammar declares required no longer default, and blank findings/claim fields classify as a retryable schema violation because silently accepting them recreated the live failure as an apparently completed pass.
Also kept: shipped-report compat (`storage.rs`'s migrations, the baseline-snapshot group defaults, the summary title default, the portability v1 archive), and the corrupt-blob loud-skip robustness.

Two facts made the cut cheaper than the inventory suggested.
Derived `Deserialize` already reads a missing `Option<T>` field as `None`, so `#[serde(default)]` on an `Option` field was a no-op — 58 of the justified sites did nothing.
Strict therefore means exactly this: a missing required (non-nullable) key fails the decode, while a nullable field reads a missing key as `None` by serde's derive semantics — presence is not enforced on nullable fields, and enforcing it would add a per-field deserializer to guard against data that no longer exists (declined at the Codex round).
Nothing in `portfolio/` uses `skip_serializing_if`, so every field is always written and a strict decode cannot break a same-build round-trip.

## What was removed

1. **The decode layer on app-written structs** — 148 `#[serde(default)]` / `#[serde(default = "…")]` attributes across `distill.rs` (`DistilledResearch`, `ResearchAuditRecord`), `engine.rs` (`RateAnchors`, `QuarterlyIncomeRow`, `QuarterlyCashFlowRow`, `ConsensusEstimate`, `CompanyFinancials`, `ComputedMetrics`, `TargetMeta`, `EngineOutput`, `QuickCheckBasis`), `fund.rs` (`FundExposureBasis`), `mod.rs` (`GradedVerdict`, `RoleRiskVerdict`, `ConditionEvalState`, `LedgerCondition`, `KeyDriver`, `ThesisLedger`, `ConditionCrossing`, `ClosedCondition`, `LedgerAudit`, `HoldingVerdict`, `DataHealth`, `PortfolioRollUp`, `ForensicRead`, `HoldingAudit`, `PortfolioRun`, `RatePrints`, `DeltaEntry`), `outcome.rs` (`FalsifierEvent`, `CalibrationSnapshot`, `ScoredLabel`, `DecisionEpisode`, `TargetCalibrationRead`, `DerivedReads`), `quick_check.rs` (`FamilySweep`, `HoldingQuickState`, `QuickCheckState`), `research.rs` (`DistilledClaim`, `ResearchSeed`, `EvidenceClaim`, `FollowupProposal`, `PassFindings`, `TopicResearch`, `HoldingResearch`) and `store.rs` (`CheckpointHeader`), with every "pre-field / persisted before the field / pre-stamp" doc clause beside them.
   The two default functions went with their attributes: `engine::evidence_floor_v1` (the audit and the checkpoint header read a missing floor stamp as the presence floor) and `store::checkpoint_format_v1` (a missing format stamp read as `checkpoint-v1`).
2. **Fields tightened to required where the producer always writes them** — `HoldingAudit.grade_parameter_version` (`String`; every audit, the early exits included), `GradedVerdict.risk_tier` / `dead_money` and the `CalibrationSnapshot` copies, `FundExposureBasis.structural_flag` (`bool`), `PortfolioRollUp.data_health`, `PortfolioRun.rate_prints` / `outcome`; the frontend mirrors in `src/types.ts` (`risk_tier`, `dead_money`, `action_rationale`, `thesis_ledger`, `action_source`, `side_reversed`, the five feed-gap counts, `model_retries`, `data_health`, `outcome`, the three scoreboard arrays, `parameter_version`, `authored_band_relation`) and the `PortfolioView.vue` guards on `data_health` and `outcome` that only a pre-field run could have needed.
   `DataHealth`, `RatePrints`, `OutcomeRecords`, `DerivedReads`, `SelfCorrectionRead` and `EligibilityRecord` derive `Default` so test fixtures build them without a literal.
   `HoldingVerdict.analyzed_at` stays `Option` on both sides: the job stamps a fresh verdict with the run's `created_at`, a carried verdict with its vintage and an insufficient-evidence exit with its prior's, so a debut abstention — certain on the wiped store's first run — persists `None` (the reviewer round caught the slice's first draft calling it always stamped).
3. **The semantic branches on a missing stamp or field** — `grade_parameter_change`'s `None` arm ("a pre-stamp prior crosses the whole history"; a missing stamp now reads as no describable boundary, like an unrecognized one) and the `"pre-stamp"` label in the delta row; the quick check's "stored basis predates the overlay-flag leg" arm and note; the "upgrade path / pre-stamp state" framing on the statement-basis adopt branch (it is the first-evaluation path), on `authored_band_relation` (`None` = no band), on the quick-check basis and bridge comments (a withheld comparator, a no-price exit), on `KeyDriver.driver_id`, on `PositionChange::Unchanged`, on `ActionSource`, on `HoldingVerdict.analyzed_at` and `effective_vintage` (the job stamps every persisted verdict except a debut abstention — item 2), and on the outcome pass's "upgrade seam" (reframed as the unseeded seam — a symbol with no readable episode at all, none ever opened or its matured history pruned, opens its debut).
   The `GRADE_PARAMETER_HISTORY` rows stay by ruling: they describe bumps that happened in code and are the fold mechanism's only real fixture until the next bump.
4. **Tests** — deleted: `verdicts_persisted_before_the_ledger_decode_with_none`, `pre_cef_role_risk_rows_deserialize_with_defaults`, `a_pre_basis_blob_decodes_with_absent_quick_fields`, `a_run_persisted_before_the_evidence_floor_stamp_decodes_as_the_presence_floor`, `a_legacy_basis_without_the_flag_degrades_instead_of_fabricating_a_transition`, `pre_overlay_audit_json_decodes_with_a_none_overlay`, the stripped-header halves of the resume-gate test (the drifted-stamp refusals stay, the foreign-format loader case now writes an explicit `checkpoint-v1` header), and the pre-stamp cases of the boundary tests (a missing stamp is now pinned silent).
   Reframed: `a_first_evaluation_adopts_the_basis_without_a_discontinuity`, `an_unseeded_symbol_with_a_prior_verdict_seeds_a_debut_episode`, `overlay_round_trips_through_json`, `a_stamped_vintage_wins_over_the_run_date` (its JSON carries the now-required keys).
5. **Portability's pre-release rungs** — `required_db_entries` keeps the v1 set (the shipped build's format) and the current set; `check_format_version` refuses a v2 or v3 manifest outright as a pre-release format no shipped build wrote (`a_v2_stamp_is_refused_as_a_pre_release_format`, `a_v3_stamp_is_refused_as_a_pre_release_format`).
   [data-portability.md §Import flow](../data-portability.md#import-flow) carries the rule.
6. **Docs** — [portfolio-analysis.md](../portfolio-analysis.md): the presence-floor read under §Evidence floor, the "pre-stamp prior crosses the whole history" and "a run decoding no stamp" sentences under §Starting parameters, the pre-field clauses on the anchor-less row, the band-relation flag, the fund comparison gate, and the episode bridge; the 2026-08-17 record's kept-class sentence is annotated as superseded.
7. **The watch set** — [big-run-watch-set.md](big-run-watch-set.md) now states that the store is wiped and every holding is a debut on run 1, so every read against a prior (the retrospective, input delta, ledger evaluation, what-changed audit, parameter-boundary NOTE, basis and equity-source gates, episode extension) is a run-2 watch, while the quick check's first sweep — run between runs against run 1's own persisted comparators — is a run-1 watch; the grade-stamp, quick-basis and pace-comparator lines are rewritten for that shape, and the pre-sweep-ledger line is gone.
   A second run follows only on the user's decision after run 1's result.

## Widening

Two outcome tests (`unfinished_price_arithmetic_takes_the_coverage_lifecycle_never_pending_forever`, `an_overflowing_benchmark_return_reads_the_leg_unavailable_with_its_gap`) derived their tiny-bar window from `Utc::now().date_naive()` while `old_episode` dates the anchor by ET session, so between 20:00 and 24:00 ET the two disagreed by a day and both failed — a pre-existing time-of-day dependence this slice's gate hit at 20:41 ET.
Both now key on `market_clock::et_date_of(&anchor_at)`; nothing under test changed.

## Stamp axis

None moves: removing decode tolerance changes neither what a completed holding's verdict or audit means nor the trail's wire shape (every removed default sat on a field the producer already always wrote).
Re-asked after the reviewer round, per group 1's lesson.

## Review rounds

Reviewer round 1 (2026-08-29): reject-with-reasons on one criterion — the frontend `analyzed_at` had been tightened to `string` on the claim that the job stamps every persisted verdict, while `job.rs` stamps every verdict except an insufficient-evidence exit, which inherits its prior's vintage and so persists `None` on a debut abstention — certain on the wiped store's first run.
Closed on the plan's own shape: the type is `string | null` again, the `analyzed_at` and `effective_vintage` docs name the debut abstention, and the stamping behaviour is unchanged (stamping abstentions at persist was the alternative and would have been a behaviour change beyond the slice).
Four nits folded in the same round: the watch set's summary-partition line no longer presupposes a second run, a re-wrapped comment line in `outcome.rs` and an orphaned one in `pipeline.rs` re-flowed, and the view's redundant parentheses and broken comment wrap removed.
Every other criterion passed: the attribute count (186 → 38, the 148 removed all on app-written structs), the writer test on every remaining default, the producers behind every other tightened field, the semantic-branch removals, the docs, the watch-set gating, the portability rungs, and the unmoved stamp axes.

Codex round 1 (2026-08-29): not approved on four findings, each verified against the code and folded in.
The watch set's first-sweep oracle was inverted — the sweep reads the latest persisted run, which after the wipe is run 1 itself, and a debut persists its quick-check basis and fund comparator (its price bridge resolves at exactly 1.0), so the first sweep reads them all and `unknown` there marks a genuinely withheld or missing input, never an expected state.
`RoleRiskVerdict.action_rationale` was still optional on the frontend with its pre-action-call comment while Rust requires it and the pipeline rejects an empty rationale on both branches; the type, the fixtures and the two view guards now match.
The record's "decodes strictly" claim is narrowed to what serde's derive enforces (the sentence above), and the presence-enforcing alternative declined.
The `FORMAT_VERSION` doc still said v2 and v3 archives import complete; it now states the refusal beside the rule, and the record's own stale `analyzed_at` parenthetical and grep claim are corrected.

Codex round 2 (2026-08-29): two residues of round 1's shapes, both folded in.
The watch set's preamble still listed the quick-check comparators among the reads that cannot fire on run 1, contradicting the corrected oracle below it; the preamble now names the quick check as the exception (its first sweep runs between runs against run 1's own persisted comparators), and item 7 above says the same.
A fourth role-risk fixture, shaped unlike the three patched in round 1, still omitted `action_rationale` — invisible to Vitest, which type-strips the specs — so it carries one now and the rationale test pins the role-risk card's render beside the priced one.

## Verification

`cd src-tauri && cargo test`: every suite green — 1,343 lib tests passed (31 ignored, the live smokes) plus the 2 / 23 / 6 / 1 integration suites, exit 0.
`cargo clippy --all-targets --all-features`: exit 0, no warnings.
`npm run build`: `vue-tsc --noEmit` and the Vite build clean; `npm test`: 46 Node tests and 249 Vitest tests passed.
`git diff --check`: clean.
The done-state greps return nothing: `serde(default` remains only on the model-wire structs (`distill.rs`, `research.rs`, the draft and interpretation types in `mod.rs`) and the persisted-before / pre-field / pre-stamp / pre-action-call / before-it-existed phrasings are gone from `src-tauri/src/portfolio`, `src/types.ts`, `PortfolioView.vue`, `portfolio-analysis.md`, `data-portability.md` and the watch set.
