# Fresh-start legacy removal — decision and removal inventory

*2026-08-17.
A code + docs slice, not a point-in-time probe: the contracts it changed live in the docs cited below.*

## The ruling

The user ruled that Portfolio Analysis will run from a **fresh, v9-only store** — no pre-`portfolio-v9` verdicts, episodes, or runs will ever exist.
The code must therefore carry **no one-time / construction-era legacy logic** kept only to decode or discriminate old data.

Scope confirmed by the user: **remove all of it** (the whole construction-era decode layer), and do it **now** as its own slice.
Boundary held: general `#[serde(default)]` forward-compat that is *not* construction-specific stays (ordinary schema evolution across binary versions — e.g. `PortfolioRun.data_health`).
Superseded 2026-08-29: that kept class was cut too, since the dev store is wiped before the big run and no local-suite data compat is required pre-release ([2026-08-29-fresh-start-2-local-suite-compat-removal.md](2026-08-29-fresh-start-2-local-suite-compat-removal.md)).
The unparseable-blob **loud-skip** robustness (`decode_run` / `PortfolioRunSummary.readable`) also stays — a corrupt row is a store-integrity concern, not construction-era legacy.

## What was removed

1. **Whole-book prior-action labeling.**
   `whole_book_era_version()` and `prior_action_is_whole_book_era()`, the two-branch action-prompt text, and the `HoldingDossier.prior_prompt_version` / `PriorHolding.prompt_version` carry that fed them.
   The prior action is now always rendered as the plain continuity baseline.
   (`AuditRecord.prompt_version` — the persisted provenance stamp — is unrelated and stays.)
2. **Construction-era decode blobs.**
   `PortfolioRollUp.aggregates` and `.construction` (`serde_json::Value` blobs kept only for shape discrimination), on both the Rust struct and the frontend `PortfolioRollUp` type.
3. **The `constructed` marker + degraded-run / "no book" machinery.**
   The `PortfolioRun.constructed` field, `has_constructed_book()`, `derived_constructed()`, `resolve_constructed()`, the `constructed` **store column** and its one-time `migrate_constructed_column` ALTER, the `latest_run` degraded-exclusion, `PortfolioRunSummary.constructed`, and `constructed_rows_exist()` (its "all-degraded vs all-corrupt" split collapsed into `any_runs`).
   The full frontend surface went with it — `types.ts` fields, `App.vue` `degradedOnlyHistory`, the `RecentReportsSidebar` "no book" tag, and every `PortfolioView` degraded-run state (`isDegradedRun`, the "No constructed run yet." empty state, the missing-book banner).
   Since `portfolio-v9` removed the step-7b construction stage, no run could ever be degraded, so this was inert on a fresh store.
4. **Construction-era episode fields and variants.**
   The `OpenReason::LeanChange` / `WeightRangeChange` variants (never produced since v9), and the `PricedEpisode.lean` / `.lean_divergence` / `.target_weight_low` / `.target_weight_high` and `RoleRiskEpisode.target_weight_low` / `.target_weight_high` decode-only fields.
   Since `lean == action` under v9, the calibration's vintage-fresh intrinsic cohort (`CohortWindowRead.lean_cohorts`) now keys on `action` directly — identical members, no behavior change; the field name is retained.
5. **The `^SPX` Stooq-era cache cleanup** at store init (idempotent one-time — nothing to clean on a fresh store).
6. **Remaining pre-v9 verdict / ledger / render backward-compat** (a Codex-round extension of the boundary — "no pre-v9 verdicts will exist" kills *backward* decode/render tolerance too, while forward-compat stays).
   The `LedgerSeries::PortfolioWeight` variant and its `retired()` skip (retired with the construction stage's target-weight range); the `VerdictDisposition` `#[serde(alias = "graded")]` (pre-union single-equity decode); and the frontend pre-v7 single-arm render fallback — the two verdict arms (`model_view` / `engine_view`) are now a required part of the rendered record, the `twoArm` guard and `.hc-body-single` column dropped.

## Round-3 extension (Codex sweep completion)

A third Codex round found the pre-v9 sweep still incomplete: the *backward* decode tolerance the round-2 boundary killed for verdicts / ledgers / renders was still present on the two-arm schema itself, one arm short of the frontend contract.
Each item below was grounded against the code and folded in the same day.

7. **The two verdict arms tightened to required in Rust.**
   `GradedVerdict.model_view` / `engine_view` dropped `#[serde(default)] Option<…>` for `ModelView` / `EngineView`.
   The frontend already typed both arms present and dereferenced them unconditionally, so the Rust `Option` (defaulted "for pre-v7 runs") was a false cross-layer contract: a missing-arm row decoded to `Some(run)`, passed the readable guard, then crashed on render.
   Tightening routes such a row through decode failure and the unparseable-blob loud-skip instead.
   The producer always supplied both arms, so this is schema honesty, not a behavior change; the pipeline consumers and the two `None`-arm test fixtures were updated to real arms.
8. **The episode calibration snapshot's six arm-fields tightened to required.**
   `CalibrationSnapshot.model_price_targets` / `model_sub_scores` / `model_outlook` / `engine_outlook` / `engine_conviction` / `engine_action` dropped `#[serde(default)] Option<…>` ("None on pre-v7 episodes").
   Under v9 both arms ride every priced episode, so the snapshot always carries them; episode creation and the head-to-head / outlook-direction reads move to direct access.
9. **The `PreProfitOverlay.clamped_from` field removed.**
   The `Option<Conviction>` recorded the pre-`portfolio-v7` model-clamp value; since v7 nothing clamps the model (the ceiling binds the engine stand-in), so the field is written `None` at its sole constructor and is always `None` under v9 — pre-v9 decode residue carrying no v9 content.
   Its two `== None` test asserts were redundant with the adjacent unclamped-conviction checks and were dropped.
10. **Test-fixture and living-doc residue.**
    The `ThesisLedger` component fixture (`tests/components/PortfolioView.spec.ts`) dropped its stale `target_weight_low` / `target_weight_high` injection — the production type lost those fields in item 4, but type-stripping had hidden the residue.
    Two living-doc references to the deleted pre-v7 single-column render fallback were corrected — `docs/portfolio-analysis.md` §Storage and display and the `PortfolioView.vue` `.hc-body` CSS comment.
11. **A retired-at-v7 dead function removed** (a fourth Codex round).
    `pre_profit::allowed_conviction_labels()` — documented "Retired with `portfolio-v7`, no production caller remains," its only consumer the `schema_labels_narrow_with_the_ceiling` test — was deleted together with that test.
    In the same pass the §The two-arm scoreboard sentence claiming a pre-v7 episode "carries neither arm and drops out of the model-arm reads" was corrected — after item 8 such an episode cannot decode at all.

The boundary held the other way too.
`#[serde(default)]` defaults whose empty / `None` is a genuine v9 state stay: the `DerivedReads` scoreboard vecs are empty only until episodes mature, the same character as the already-kept `PortfolioRun.data_health` and `.exited`.
The distinguishing test is whether the field ever takes a value other than its default under the v9 producer — the removed arms and `clamped_from` never do; the kept vecs and `data_health` do.
Their comments were reworded off the "pre-v7" framing so they no longer read as leftover backward-compat.

A fifth Codex round found three more explicit backward/migration paths and two more stale docs; all were verified against the code and removed.

12. **The `PriceTargets` rename aliases removed.**
    `#[serde(alias = "end_of_month")]` / `end_of_year` on `one_month` / `twelve_month` decoded runs persisted under the pre-rename field names — backward-facing decode with no forward role, the same character as the item-6 `graded` alias.
    On a fresh store no run ever wrote the old names.
13. **The ET-dating self-heal removed.**
    `mature_labels` re-derived and re-stamped a pending window end when the stored string disagreed — documented a "one-time self-heal per legacy episode" for an episode persisted before ET dating (pre-v9), inert on a fresh store where every episode is ET-dated at open.
    Its dedicated `legacy_utc_keyed_pending_windows_re_key_to_the_et_anchor` test went with it; the live maturation-persistence flag it shared (`summary.changed`, set when a label matures) stayed.
    (This one carried incidental forward-value as a window-end re-derivation guard should the derivation logic ever change; removed per the ruling, trivially restorable if that insurance is wanted.)
14. **The confirmed-crossing `confirmed_at` legacy fallback removed.**
    A confirmed falsifier crossing with no `confirmed_at` fell back to the consuming run's ET date — the old behavior for a legacy eval state that confirmed before the field existed.
    Under v9 the engine stamps `confirmed_at` on the confirming pass with the run's ET session date (`run_date`, ET-derived in `job.rs`), so a confirmed crossing always carries it and the consumer reads it directly; a confirmed-but-unstamped crossing (impossible on a fresh store) is now skipped rather than dated by guess.
    The `confirmed_at` field itself stays `Option` — it is legitimately `None` on a not-yet-confirmed breach.
    Four attachment tests carried unrealistic `confirmed_at: None` fixtures on confirmed crossings (they leaned on the fallback for dating) and were given the ET stamp the engine would write; the fallback-specific `a_legacy_crossing_without_a_confirmation_date_keeps_the_old_stamp` test and the `confirmed_crossing` helper's dead `None` branch were removed.
15. **Ledger doc — the `portfolio-weight` decode-and-skip claim.**
    `portfolio-analysis.md` §Action triggers said the retired series "stays decodable on persisted conditions but evaluation skips it whole"; item 6 removed the `LedgerSeries::PortfolioWeight` variant and its skip outright, so it was corrected to say the series was removed rather than retained as a decode-and-skip path.
16. **Frontend type doc — the `graded` alias claim.**
    `types.ts` said legacy `graded` rows re-serialize as `priced`; item 6 removed that `#[serde(alias)]`, so the parenthetical was dropped.
    Two pipeline prompt-section fixtures and one `HoldingVerdict.what_changed` doc comment that still narrated the removed fallback / older-run decode were de-staled in the same pass.

A sixth review round caught three residual doc claims describing removals already made, corrected in place: `types.ts` and `portfolio-analysis.md` §Starting parameters both still said `PriceTargets` decodes the `end_of_month` / `end_of_year` names through serde aliases (removed in item 12), and the `ConditionCrossing.confirmed_at` doc still listed a legacy already-confirmed state as a `None` case (removed in item 14 — `None` is now `FirstBreach`-only).

## Verification

Backend `cargo test` (all suites, 0 failed) and `cargo clippy --all-targets --all-features` (warning-free).
Frontend `npm run build` (vue-tsc + Vite) and `npm test` (233 component tests + the pure-module tests).
Removed the tests that only exercised the deleted machinery; kept and re-pointed the unparseable-blob robustness tests.

## Docs aligned in the same slice

`portfolio-analysis.md` (§Failure posture, §Storage and display, retention, §Outcome learning episode contents and cohort layers), `storage.md` (§Local Analysis Suite Storage), `interface.md` (§Main Layout), `portfolio-workflow.md`, and `logic-flow-docs/portfolio-analysis-logic-flow.md`.
`.metis/BUILD.md` and `.metis/INDEX.md` still describe the removed machinery as live and need a user-run update (the construction-era-legacy runtime paragraph, the episode `lean` / `lean_divergence` note, and the degraded-run INDEX row).
Dated verification records that reference the degraded-run machinery are point-in-time history and were left as written.
