# Portfolio financial-correctness sweep and review follow-up

## Scope and authority

The user requested a financial-correctness review, excluding improvements and optimizations, then authorized fixing the four reported issues.
Claude Code independently reviewed the implementation and confirmed the four financial corrections while identifying pre-release compatibility, unused-field and documentation problems.
A second review confirmed those corrections and requested the final record and documentation cleanup below.
This record describes the approved implementation and the follow-up verification.

## Financial corrections

| Finding | Final behavior | Canonical contract |
|---|---|---|
| Currency and ADR share-unit mismatch | Statement and listing unit guards stop unsupported pricing | [Asset eligibility](../portfolio-analysis.md#asset-eligibility) |
| Option-overlay payoff omitted from equity targets | Identified option-overlay funds use role/risk analysis | [Asset eligibility](../portfolio-analysis.md#asset-eligibility) |
| Annual-authored flow thresholds compared with fresh TTM | Incompatible filing-flow comparisons are withheld without advancing state | [The position thesis ledger](../portfolio-analysis.md#the-position-thesis-ledger) |
| Model outlooks scored at the engine's different horizons | Each scored forecast uses its authored horizon; the UI compares matching horizons | [Outcome learning](../portfolio-analysis.md#outcome-learning-calibration) |

## Rulings — 2026-09-15

The user accepted the reviewed fixes and requested completion of the remaining pre-commit items.
The retained decisions are recorded here with that approval:

- **Option-overlay funds: role/risk only.**
  An annotated equity bull target would still supply an unsupported payoff as evidence and as a displayed figure.
  These funds therefore receive role, risk and an action without a letter or targets.
- **Depositary receipts: excluded until financial units can be verified.**
  The exclusion applies even to USD-reporting or one-to-one receipts because the provider's per-share convention is unverified.
  The way back is a deferred dated-FX and verified ADS-ratio normalization slice, recorded in BUILD under Owned by no slice.
- **Outlook scoring: match the forecast horizon.**
  Model mid scores at twelve months and model long remains unscored.
  The engine's 1/6/12-month stand-in retains its trailing-return basis, while the model authors forward calls.
  Their comparison matches window length rather than forecasting method.
- **Version stamps: retain the final set below.**
  The prompt stays in the unrun v35 bundle.
- **Pre-release compatibility: remove the older v1/v2 quick-state migrations too.**
  They have no consumer under the clean-store rule.

## Claude-review corrections

- Removed the new quick-check migration and audit-floor blocking branches, their tests, the nullable direction-scoring version field, the UI legacy-score gate and its test, and the compatibility section added to storage documentation.
  At the same quick-state migration seam, removed the pre-existing v1/v2 migration branches and serde legacy defaults from the quick state and checkpoint header, following the [pre-release rule](2026-08-29-fresh-start-2-local-suite-compat-removal.md).
- Removed the unused structural flag from the priced engine output, persisted graded verdict, frontend type, producers, fixtures and priced-card tag.
  The role/risk verdict and exposure-basis flag remain active.
  Removed both unreachable structural inputs from the priced-fund tier function.
- Corrected the FMP regression fixture to pass the actual TTM-adoption result into the merge.
- Consolidated the unit rule at Asset eligibility, replaced the other full copies with pointers, and added option-overlay funds to both unpriceable-class lists.
  Updated the tier contract, watch set, BUILD, INDEX and current handoff.

## Final pre-commit cleanup

Aligned Starting parameters with BUILD: horizon matching is a contract, while the engine lookback windows remain drafted.
Described the independent reviews inline so the record has no dependency on a gitignored handoff file.
Added the deferred ADR-normalization work to BUILD and refreshed the Metis handoff to the approved, uncommitted state.
Matched the watch set's plain-line format and split the new joined prose claims.
The quick-check state's own parameter stamp remains recorded without a mismatch consumer.
The checkpoint header copy gates resume, and a post-release state-mismatch policy remains outside this pre-release slice.

## Version contract

The final stamp set is `portfolio-v35`, `checkpoint-v9`, `evidence-floor-v5`, `quick-check-v4`, `grade-v2.3`, `targets-v6` and `pre-profit-v4`.
The prompt wording stays in the unrun v35 bundle.
The floor and quick-check axes describe the changed financial semantics.
The checkpoint format moves because the priced verdict's persisted shape lost a field.
No data migration or blanket quick-check suspension is introduced.
The existing checkpoint mismatch gate and the pre-release clean-store policy remain the contract.

## Verification

Independently rerun after the Claude-review corrections:

- `cd src-tauri && cargo test`: 1,449 library tests plus 32 integration tests passed.
  The 31 existing ignored tests were not run.
- `cd src-tauri && cargo clippy --all-targets --all-features`: passed without warnings.
- `npm run build`: type-check and Vite build passed.
- `npm test`: 46 Node tests and 255 Vue component tests passed.
- `git diff --check`: passed.

Regression coverage includes the USD/TWD valuation counterexample, null and mixed statement currencies, ADR metadata, otherwise complete financials with a unit issue, option-overlay distributions, persisted pipeline outputs without targets, Annual and unstamped flow conditions, the valid TTM comparison control, and model-mid scoring at twelve months without a model six-month read.
The reduced counts reflect removal of obsolete compatibility tests, not failed or skipped regressions.
The [watch set](big-run-watch-set.md) now covers missing statement currency, the ADR/overlay exclusion counts, and the first-sweep flow-withhold rate.
No live analysis, provider probe, store wipe, commit or launch was performed.
