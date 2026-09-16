# Ledger-validator follow-up — fix list 1.5–1.8 and 2.4 (2026-09-16)

## Scope and authority

The small follow-up slice ruled 2026-09-16 off the §8.2 live admission read (`2026-09-16-ledger-conditions-and-action-packet.md` §Live admission read): five entries the read raised, one plan, one prompt stamp (`portfolio-v37`), then a fixed-set re-read that admits fix list §1.
Contract text belongs in the job docs named per entry; this record carries the rulings, the inventory and the re-read.

## Rulings — 2026-09-16 (plan flags and assumptions)

- F1 — A price statement naming a percent from current levels resolves that percent against the spot the caller passes and checks it within the price tolerance; a statement naming no figure at all is `no-level` on every series.
  Ruled 2026-09-16: convert against the current price.
- F2 — "confirming" leaves the second-condition list; a genuine second test with its own level stays caught by the multiple-level rule and a volume clause by the volume rule.
  Ruled 2026-09-16: remove the word entirely.
- F3 — The relative margin cap is 25% of the level on the price, the multiples and the debt / equity ratio, and 50% on the fraction-unit series, applied after the magnitude bound under `margin-implausible`.
  Ruled 2026-09-16: adopt those fractions.
- F4 — The action packet's capital-efficiency line states the hurdle fact and closes with "This line neither requires nor forbids any rung." on `clears`, `indeterminate` and `unscorable`; `fails` is unchanged.
  Ruled 2026-09-16: adopt that wording.
  Corrected after Codex's review (finding 4): the drafted facts were wrong about the engine — `clears` is "even the bear case clears the hurdle" and `indeterminate` is "the bear case misses the hurdle and the bull case clears it, so the read proves nothing either way" (`engine::hurdle_read`); the ruled closing clause stands.
- A1 — A duration keeps only when it names the market-data cadence exactly: two days, sessions or closes; weeks, months and quarters never match, and a filing series never exempts.
  Ruled 2026-09-16: adopt.
- A2 — A parenthesized figure is a restatement, not a second level, only when it directly follows a stated level.
  Ruled 2026-09-16: adopt.
- A3 — `portfolio::PROMPT_VERSION` moves to `portfolio-v37`; `store::CHECKPOINT_FORMAT_VERSION` stays `checkpoint-v9` (no persisted shape changes); this record is the slice's own file.
  Ruled 2026-09-16: adopt.
- A4 — The re-read runs three repeats with thinking captured.
  Ruled 2026-09-16: adopt.
- F5 (raised at implement time) — The split bridge re-bases a prior price core at ingestion but left its sentence on the old basis, so the level check downgraded a carried "$700" beside a 175 core; latent since the v36 level check, masked by test fixtures that stated no figure.
  Ruled 2026-09-16: the ingestion normalization also re-bases the dollar figures in a price condition's statement (`rebase_dollar_figures`); canonical at `portfolio-analysis.md §Starting parameters` (Split-adjustment bridge).

## What landed

- `pipeline::validate_quant_core` (`portfolio-v37`): a statement naming no figure in the series' unit downgrades as `no-level` on every series (1.5); a price statement naming a percent from current levels resolves it against the spot the rewrite passes, the sign from the sentence when explicit and from the comparator otherwise (F1); the spot threads through `validate_condition` from the rewrite.
- `duration_clause` replaces `names_duration`: it parses the count and unit, tolerates up to two adjectives between "consecutive" and the unit, knows "close", and a duration naming the market-data cadence exactly keeps its core (1.6, A1).
- "confirming" leaves `CONJUNCTION_CLAUSES`; `anchored_levels` treats a parenthesized figure directly after a stated level as a restatement, for both the level check and the multiple-level rule (1.6, F2, A2).
- `MARGIN_CAP_PRICE_RATIO` (25%) and `MARGIN_CAP_FRACTION` (50%) sit beside the magnitude bound under `margin-implausible` (1.8, F3).
- `LedgerSeriesContract::render` states that the threshold is exactly the level the statement names and the margin the separate band (1.7).
- The action packet's `clears`, `indeterminate` and `unscorable` capital-efficiency lines state the hurdle fact and that the line neither requires nor forbids any rung; `fails` is unchanged (2.4, F4).
- `rebase_dollar_figures`: the full pass's ingestion normalization re-bases the dollar figures in a price condition's statement with its core (F5).
- `portfolio::PROMPT_VERSION` → `portfolio-v37`; `checkpoint-v9` and every other axis unchanged.
- Tests: the 6g class table gains the live read's cases (eight downgrades, eight keeps, the spot-resolved percent form), the margin-guard test moves to the cap boundary, the statement re-basis has its own case, and the synthetic ledger drafts across the pipeline tests now name their figure in the series' unit (the shared stub trigger included), since a levelless sentence no longer validates.
- Docs: `portfolio-analysis.md §The position thesis ledger` (the four new class sentences and the contract sentence) and §Starting parameters (the split bridge's statement re-basis); `portfolio-workflow.md §Step 6g` (the v37 pointer).

## Verification

Offline, at implement time:

```
cd src-tauri && cargo test          # 1479 passed, 0 failed, 32 ignored (the workspace's other targets green)
cd src-tauri && cargo clippy --all-targets --all-features   # 0 warnings, 0 errors (full output read)
npm run build                       # vue-tsc + vite green
```

Targeted: `cargo test six_g_` (the class tables) and `cargo test split_rebasis` (the statement re-basis and the two bridge tests it un-broke).

Task review (2026-09-16): every code criterion passed on the first round.
Codex's review followed (§Codex implementation-review corrections); the gate below was re-run after its five fixes.
The one reject was a doc contradiction — the split-bridge passage's "never rewritten by the app" sentence beside the new statement re-basis — fixed by restating the one rule.
Three nits were taken: a semicolon split in the ledger section, a v37 sentence on the stamp test's comment chain, and `{:.0}` on two test statements.
The second round approved with one nit on this paragraph's own shape, taken here.

## Codex implementation-review corrections

Read-only review after the task review; Codex changed no files and ran no gate, so the full gate ran here after the fixes.
Five findings, all P2, all verified against the code and all fixed:

1. A parenthetical naming another metric ("below $200 (with gross margin below 15%)") was folded into a restatement and its condition dropped; a parenthetical is a restatement only when it names no engine series or off-surface metric (`names_any_metric`).
2. Any percent in a dollar-less price statement resolved against the spot, so "as gross margin falls below 20%" at a $100 spot validated a price core of 80; the resolution now takes only a percent anchored to the current level ("from current levels", "below its current") or whose nearest move verb has the price as its subject (`relative_price_figures`); the earlier record note that such a case merely lands under another class was wrong and is withdrawn.
3. Only the first duration clause was read, so a cadence-exact clause hid a later unsupported one; every clause is collected and each must be the cadence (`duration_clauses`).
4. The capital-efficiency lines misdescribed the engine's states (F4 above).
5. The statement re-basis kept the figure's own decimals, so "$3.5" at 4:1 rendered "$0.9" against a 0.875 core, outside the 1% tolerance; decimals now grow until the rendering sits within 0.1% of the converted value ("$0.875").

A second Codex round found two more in the relative-price parser, both fixed:

6. The price-subject branch could never pass, since the price's own aliases counted as "a metric" in its span ("Share price drops 20%" downgraded); the subject test now excludes the price series (`names_non_price_metric`) while the parenthetical test keeps it.
7. The from-current anchor never checked what the percent described ("as gross margin drops 20% from current levels" resolved to a price level); a non-price metric in the six words before the figure now rejects it under either branch.

## Known edges (accepted as written)

- A parenthetical alternative that names the price ("below $200 (or above $250)") is a second level and downgrades as `qualifier`; one that names no metric at all still reads as a restatement.
- A bare "margin" or another metric noun outside the alias lists does not block the relative-price resolution; the lists are the lexicon.
- The statement re-basis moves every dollar figure in a price condition's sentence and drops thousands separators.
- A bare decimal on a fraction series ("below 0.03" with no percent sign) is no figure under the docs' own-unit definition and downgrades as `no-level`.

## The fixed-set re-read

Pending.
