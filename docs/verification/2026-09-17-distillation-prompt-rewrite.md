# The distillation prompt rewrite — `portfolio-v44` (2026-09-17)

## Scope and authority

The `portfolio-v40` interpretation rewrite set the principle: a prompt never explains the app or its concepts to the model (`2026-09-17-interpretation-prompt-rewrite.md`).
The action, role/risk and research rewrites carried it to their calls (`portfolio-v41` to `portfolio-v43`).
The four distillation calls — the pass, tier-1, tree-reduce and reduce prompts in `distill.rs` — were the last pre-v40 shape on the Portfolio local-model surface, each a bare user message with the task in its first sentence, instructions on every heading and the pre-v40 template.
The audit read them beside attempt 6's persisted distillation output on six holdings (`~/Downloads/market-signal-attempt-6-logs/store-extracts/*_row.json`, `audit.research`).
The claims rendered with retrieval timestamps and nothing else, so the model dated facts by retrieval: ARKF's findings say the listing change was "announced on September 16, 2026 (with trading expected March 31, 2025)", and TSLA's forward figure carries the retrieval date as its as-of.
Every typed field was asked for on every holding and filled regardless of support: an invented driver id on three of four leading indicators (the first-analysis and fund calls render no driver list), a capital-expenditure figure the engine cannot recompute, a fraud claim citing a news site, and confidence 1.0 on every field.
On a priced fund no typed field has a consumer, yet all were asked for.
The `conflict_handling` declaration changed only the rejection's wording: with a feed value present both declarations rejected, without one both filled as a supplement.
The backfill record was in the grammar on every overlay-eligible call, though the research asked the backfill question only when the obligation bound.
One object per topic was never stated, though a topic the model omits deletes its stored seed row.
The two lines a validated typed field puts into the interpretation packet still narrated the app, and the fixed set carries neither, so the v40 pin had never read them.
The user asked for the prompts audited and adjusted; the audit and the aligned draft were written beside today's render (`~/Downloads/market-signal-fixed-evidence-2026-09-16/distillation-prompt-rewrite.md` and `distillation-prompts-v43-tsla.md`), the draft's eighteen rulings and the plan's ten assumptions and flags taken through the selector.
Fix list 4.8 is the entry; this record carries the rulings, the inventory and, once read, the admission.

## Rulings — 2026-09-17

The eighteen on the prompt text:

- **The frame: two-part user message plus the role-line system prompt** on all four calls (today's calls carried no system message).
- **The holding header on every message**, through a `holding_brief` input.
- **Dates: retrieval timestamps dropped** from every claim line; prior findings dated by their analysis; SOURCE TEXT pages dated by publication where the search reported one; the task orders facts by period and publication; fix list 5.2 stays deferred.
- **One object per topic stated**, the not-searched topics included, the topic keys in the shape.
- **The shape: placeholder-only** through `placeholder_shape`, `response_shape_contract` retired from every prompt.
- **Typed fields on the stock branch only**; a priced fund takes the consolidation form.
- **The forward assumption narrowed** to `affects` eps or revenue, `fact_type` guidance, contract or filing, `as_of` the date stated.
- **`conflict_handling` dropped**; the engine reads every fact as a supplement fill.
- **The leading indicator asked only where KEY DRIVERS render**, the driver ids in the shape; none on a first analysis.
- **`confidence` dropped** on the forward assumption, the indicator and the fraud record; kept on the pre-profit rows.
- **The fraud record's eight regulator and court sources named as data**; the advisory, hard-trigger and tier-0 narration gone.
- **`backfill` asked only when the obligation bound**, through a `backfill_required` input.
- **The pre-profit row rules restated as requirements**; the admission filter unchanged.
- **`related_condition_id` only where STANDING CONDITIONS render**; the keep-your-tie sentence dropped (the app inherits).
- **The two interpretation-packet lines rewritten as data** in this slice.
- **The tree-level reduce renders parsed pass outputs** as summary and claims lines, the raw body only where one did not parse.
- **SOURCE TEXT in the research's form**: the quoted-material gloss, the publication date per page, the continuation marker, a closing count of pages not shown.
- **Stamp and admission: `portfolio-v44` and `checkpoint-v11`**, portability format 7 unchanged; a distillation section in the harness dump with the two-part and banned-word pins; admitted on the next book-scale attempt's first holdings.

The ten from the plan:

- **A1, the alternatives ride the grammar** as enums (the topic keys, the condition ids with null, the driver ids); the validators unchanged.
- **A2, `consolidation_only` replaces `role_risk`** on the inputs, true for role/risk and for every fund.
- **A3, `fact_type` and `affects` stay strings**; the grammar's enum is the only narrowing.
- **A4, the publication date renders as given, capped**; a page without one shows no date.
- **A5, a prior's date is its vintage's date part**; no new field.
- **A6, `confidence` stays on the pre-profit rows**; their admission contract untouched.
- **A7, portability format 7 stands**: the archive enumerates none of the audit's typed fields (confirmed by grep at implement).
- **F1, the Trade Opportunities record tuples follow the built producer**, with a one-line note each.
- **F2, the fund form renders on the synthetic BND sample only.**
- **F3, BUILD §Built and INDEX rows stay user-run.**

## What landed

- `HoldingResearch.page_published` (normalized URL → the search's published string, capped at `PUBLISHED_CAP_CHARS`), filled from `run_holding`'s page metadata; `DistillInputs` gains `holding_brief`, `consolidation_only` (in place of `role_risk`) and `backfill_required`, set by `run_research_and_distill` from `holding_header`, `role_risk || dossier_is_fund` and `triggers.pre_profit_backfill`.
- `ResearchForwardAssumption` loses `confidence` and `conflict_handling` (the `ConflictHandling` enum with it); `ValidatedLeadingIndicator` and `ForensicEventClaim` lose `confidence`; the three validators lose their range checks; the pipeline's Step-6e input reads `supersede: false`.
- Per-call grammars: `claim_schema(condition_ids)` carries `related_condition_id` only with ids, as a nullable enum; `topic_schema` enumerates the topic keys; `combined_schema(&ReduceShape)` carries the forward assumption (`fact_type` and `affects` as enums) and the fraud record on a typed call, the indicator only with driver ids (its `confirms_driver_id` an enum of them), the observations on an overlay-eligible call, the backfill record only when required.
  A call is typed when it is not consolidation-only and a page carries body text.
- `placeholder_shape` is `pub(crate)` with `DISTILL_KEY_ORDER`; `response_shape_contract` is test-only, the source `response_template_samples` reads for the interpretation decode test.
- The four messages are a `DistillPrompt { system, user }`, the system line from `system_prompt(scope, outputs)` naming the outputs the grammar carries.
  Part 1 opens with HOLDING (the brief), then STANDING CONDITIONS (`- <id> — Falsifier|Trigger: <statement>`) and, where the indicator is asked, KEY DRIVERS (`- <id> — <name>`).
  The TOPICS gloss carries each clause only where the section carries the thing glossed.
  A topic renders as `TOPIC <key> — <title>` with `Search N:` / `Claims:` (`- <claim> [<url>]`) and `Prior findings (analysis of <date>):` (`— bears on <id>`); a dormant prior as `TOPIC <key> (not searched this time)`.
  The hierarchical reduce renders each topic as `Summary:` / `Claims:`; the tree-level reduce renders a parsed pass output as `Search N (summary):` / `Claims:` and an unparsed body under `Search N:`.
  CONTRARY EVIDENCE follows, then on a stock's final call SOURCE TEXT (`=== <url> (published <date>) ===`, `[the page continues beyond what is shown]`, `[N further pages were retrieved but are not shown]`), bounded as before.
  A stock whose research retrieved no page body takes the consolidation form and records the gap "no original source text available; the call carried no typed field".
  Part 2 is `reduce_task` or `topic_task` in output order, closing with the RETURN SHAPE.
- `DistillModel::distill_call` takes the `DistillPrompt`; the live adapter sizes both messages at the issue guard and sends system plus user; the three implementations follow.
- The interpretation packet's two lines: `render_leading_indicator` ("Leading indicator: <metric> = <value> (<direction>, as of <date>)[, bearing on the driver "<ledger name>"]; source <url>.") and `render_fraud_record` ("Fraud record: a <host> document dated <date> names <issuer> in a fraud matter; source <url>. Whether it concerns this holding is not established.").
- Stamps: `PROMPT_VERSION` → `portfolio-v44` with its history paragraph; `CHECKPOINT_FORMAT_VERSION` → `checkpoint-v11`; portability format 7 stands.
- The harness: `distill::samples::messages` (the seven calls over hand-written TSLA research and the synthetic fund's one topic), the pin `distillation_messages_are_two_parts_with_no_app_concept`, and the dump's section 10.
- Docs: `portfolio-workflow.md` §Step 6d and §Step 6e, `storage.md` §Local Analysis Suite Storage, `trade-opportunities-workflow.md` §Step 5c and §Step 5e, `local-models.md` §Prompt posture, `web-research.md` §The research loop and context management, the fixtures README; fix list 4.8, 5.1's note and the deferred allocation watch.

## Task-review corrections

The task review returned approve-with-nits; every ruling and plan criterion passed with evidence, the gate was re-run green by the reviewer, and the nine nits were taken through the selector (2026-09-17).

1. A stock whose research retrieved no page body records the gap "no original source text available; the call carried no typed field" at the typed decision; the branch inside the source-text renderer that could no longer fire is gone, and a test pins both halves.
2. The engine's doc on the forward-assumption input's `supersede` member reads app-set `false` since `portfolio-v44`, the leg dormant until the channel is promoted.
3. The continuation marker's doc cites ruling 17 and the v43 plan's A5 for its wording.
4. The tree-level reduce renders an unparsed pass body under `Search N:`, the `(summary)` label kept for a parsed output.
5. The source-text renderer's reserved closing-count width carries a comment saying it is reserved on every call.
6. `condition_ids` stays recomputed inside each message builder, so each stays callable from the samples and tests with the inputs alone (left as is).
7. `docs/web-research.md`'s distillation sentence is a one-line pointer to §Step 6d, the contract's single home.
8. The §Step 6d indicator sentence and this record's four-messages bullet are split into sentences that stand alone.
9. This record's dump sentence names all four differences from the draft.

## Codex implementation-review corrections

Read-only review after the task review; Codex changed no files and ran no gate, so the full gate ran here after the fixes.
Two findings, both verified against the code and the docs, taken through the selector and fixed (2026-09-17):

1. The source-text renderer required room for its gloss alone and appended the closing count line even when the reservation did not fit, so a 282-character allowance rendered 333 characters and the seam's guard refused the call.
   Fix: the whole framing — the gloss and the reserved count line — must fit before anything renders, else the no-room gap; a test asserts the section never exceeds its allowance across every budget from 0 to 600 with three pages.
2. Three tuples still named the dropped fields: the §Step 5e fraud record in `trade-opportunities-workflow.md`, and the logic-flow mirror's forensic record and forward assumption.
   Fix: all three follow the built producer's form with the one-line note the §Step 5c and §Step 5e tuples carry (plan ruling F1).

## Known limits

- The distillation prompts cannot be exercised by the fixed-set live harness, which issues interpretation and action calls only; the admission read is the next book-scale attempt's first holdings against §Verification.
- The shape renderer orders every object's keys by one global list, so the backfill object shows `units` and `period_span` before `issuer_scope`, and a single-alternative enum renders as `<fraud>`; the draft showed `"kind":"fraud"`.
- A dormant prior topic renders without its title, since the persisted topic object carries the key only.
- A typed call whose SOURCE TEXT finds no room under the issue budget still asks the typed items (the section is absent and the gap recorded), as the pre-v44 prompt did.
- The engine's supersede leg stays in the policy, dormant: with no declaration every fact enters as a supplement, and a present feed value rejects the fill as before.

## Verification

Offline, at implement time:

```
cd src-tauri && cargo test
cd src-tauri && cargo clippy --all-targets --all-features
npm run build
cd src-tauri && MARKET_SIGNAL_LOCAL_EVAL_PROMPT_DUMP=~/Downloads/market-signal-fixed-evidence-2026-09-16/prompts-v44.md cargo test --lib fixed_evidence_prompt_dump -- --ignored
```

The dump's section 10 matches the ruled draft's messages line for line, four differences aside: the harness header (name unavailable, the harness date), the two shape-order limits above, the dormant topic's missing title, and the TOPICS gloss reading "what its searches established" for the draft's "what each search established".

The admission read, on the next book-scale attempt's first holdings, only when the user launches one:
no distilled finding or topic summary dates a fact by its retrieval, and no persisted `as_of` is the run date;
every typed field returned cites a page under the message's SOURCE TEXT, and none is returned on a fund;
no leading indicator names a driver id outside the rendered list, and none is returned on a first analysis;
the forward assumption, where returned, is an EPS or revenue figure from guidance, a contract or a filing;
`unreconciled_topics` stays empty and every topic key returned is one shown;
the two typed-field lines under RESEARCH SUMMARY carry no banned word;
and the distillation calls' prompt sizes are read beside attempt 6's (15,595–78,045 characters).
