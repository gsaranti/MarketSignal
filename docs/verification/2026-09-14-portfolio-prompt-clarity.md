# Portfolio prompt clarity — 2026-09-14

The user authorized implementing the combined Claude Code and Codex recommendations after reviewing the Portfolio prompts and historical thought traces.
This is an implementation record, not a claim of measured runtime improvement.

## Implemented decisions

- Synthesis cites pass-local source identifiers, resolved by the app to the existing persisted final URLs and retrieval timestamps.
  Only sources actually rendered in the synthesis packet resolve; redirected seed lineage remains intact.
- A dedicated synthesis orientation retains the prior assertions needed for disconfirmation, with no gathering-stage search instructions or prior-claim URL roster.
  The output is described positively as the object shown, and technology follow-ups use the existing competitor/supplier product-or-standard event definition.
- Interpretation, role/risk and every distillation shape show schema-derived templates with populated nested objects, array items, enums and nullable alternatives.
  Templates illustrate structure; their values prescribe no finding.
- Holding prompts name the authoritative per-share quote, and role/risk interpretation names the daily volatility from the ledger's short history separately from annualized observable risk.
  Observed data and model-authored monitoring thresholds are explicitly distinguished.
- Priced and role/risk action packets state only their applicable decision basis.
  The actual hurdle result is rendered as a fact; `fails` retains the independently-poor-prospects condition, and `clears` / `indeterminate` supply no exit signal.
  The engine's selected action remains undisclosed.
- Tax is consistently an optional caveat with no effect on the action, including the shared Settings profile label.
  It may accompany the investment reason in the same rationale sentence; no new UI or persisted field is required.
- Action continuity receives the prior read and validated current change attribution with its input-delta evidence.
  Missing attribution is explicitly unavailable; interpretation permits an identified self-correction without inventing an external change.
- Final distillation receives a bounded section of original fetched source text for exact extraction.
  Both single-pass and hierarchical routes receive it, with omitted/truncated sources reported as coverage gaps; existing source validators remain intact.

## Compatibility and limits

These changes fold into the never-run `portfolio-v35` contract under the standing pre-debut rule.
The synthesis model-wire claim changes from `source_url` to `source_id`; persisted research claims remain URL-based.
Persisted verdicts, other version axes, the engine's decisions and the model's unrestricted action ladder retain their existing contracts.

Sampling and thinking modes remain unchanged.
An A/B comparison of sampling or non-thinking synthesis is deferred; no Portfolio run, external research, or live model comparison was launched for this change.
The historical thought traces establish observed ambiguity but cannot measure the effectiveness of these revisions.
The combined improvements mean the next v35 observation is no longer an isolated test of the original synthesis shape example.

## Verification

Regression coverage exercises nested examples, source identity and redirect lineage, synthesis orientation, source text on both distillation routes, source-budget degradation, daily-volatility window fidelity, all hurdle states, tax wording and action continuity.
Initial implementation checks passed: `cargo test -- --quiet` (1,444 unit tests and 32 integration tests passed; 31 ignored), `cargo clippy --all-targets --all-features` without warnings, `npm run build`, and `npm test` (46 pure-module tests and 255 component tests).
Tracked and new-file whitespace validation passed.

## Claude review follow-up — 2026-09-14

The user authorized the necessary corrections and explicitly approved keeping tax as a caveat with no effect on the action.
The narrowed tax contract is now an explicit ruling rather than an inferred clarification.

- Source IDs are assigned only when usable evidence is admitted for rendering; the same URL-to-ID mapping supplies the header and citation resolution.
  Conservative header sizing reserves the largest possible identifier before selection, keeping the input guard valid across omitted middle pages and digit boundaries.
- Schema templates use neutral outlook samples and named placeholders for other multi-choice enums, with the allowed values listed explicitly.
  Neither `trim` nor medium confidence is treated as neutral; tests materialize every enum alternative through the real interpretation and distillation decoders.
- The priced action packet renders capital efficiency once, as its actual result and consequence.
  The extra letter-to-action mapping sentence is removed; the existing full-ladder and engine-set contracts remain unchanged.
- Synthesis distinguishes absent prior assertions, omits blank follow-up rationales, restores per-field truncation markers, and reports unresolved source IDs accurately.
  Portfolio-specific prompt mechanics now live in the workflow documentation rather than the general prompt-posture doctrine.

Original-source extraction allocation is unchanged.
Its approximately 79k-character maximum at the current widest context is a budget ceiling, not typical usage or a measured token count; added source text can route a call beyond the fast tier and can truncate relevant passages.
Evidence selection and runtime throughput remain separate follow-up work, and no live model run was launched.

Follow-up verification passed: `cargo test -- --quiet` (1,446 unit tests and 32 integration tests passed; 31 ignored), `cargo clippy --all-targets --all-features` without warnings, `npm run build`, `npm test` (46 pure-module tests and 255 component tests), and `git diff --check`.
