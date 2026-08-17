# Selective-run safety additions → card badges (2026-08-16)

A ruling and its build inventory.
It supersedes two shipped contracts: the selective-run automatic safety
additions (`docs/portfolio-analysis.md` §Triggering) and the one-time pre-`v9`
migration force-include (shipped in the tunnel-vision slice, `784c1e9`).

## The ruling

A selective run analyzes **strictly the user's selection** — nothing else is
pulled in.
The former automatic safety additions no longer force-include the holdings they
flag; each instead surfaces as a **non-blocking card badge** on the latest
result, so the user sees what changed and decides whether to re-run.
The affected triggers are the engine-only quick check's attention flags, an
`unknown` signal family, an unexamined evidence event, a net side reversal, an
over-age carried exit action, and a pre-`portfolio-v9` prompt stamp.

Three consequences fix the contract:

- **A held position with no prior verdict to carry** (a new or never-analyzed
  holding) is left **not analyzed** in a selective run rather than force-included
  — it renders as a placeholder "run to grade" card, selectable so the next
  selective run can grade it.
  A full run still grades the whole book.
- **A compromised carry stays visible, badged.**
  A side reversal marks its carried verdict `side_reversed` (the carried thesis
  is for the opposite position) and a stale exit stands behind the existing
  stale-vintage badge; the user chose "show carried advice + badge" over
  withholding it.
- **The pre-`v9` migration gate is removed, not converted.**
  The app is unreleased, so no production store holds pre-`v9` data; the only
  pre-`v9` verdicts live in the developer's dev store, and the next run is the
  full big-confirmation run, which re-grades the whole book under `v9`.
  A pre-`v9` verdict now carries like any other.

## Why

Selective runs are a rare, "I need this one holding refreshed now" convenience.
Auto-expanding a one-holding request into the whole flagged tail — especially an
`unknown` family fired by one flaky data fetch across many holdings — blocks the
urgent result behind slow local-model work the user did not ask for.
Badging aligns with the suite's own **warn-don't-decide** posture (the cheap
quick check warns; only a full pass decides), and post-tunnel-vision there is no
whole-book roll-up to keep honest, so the integrity argument for force-including
is weaker than when it was written.

## What the badge surface reuses

The engine-only quick check already sweeps the carried tail and persists its
attention flags / evidence-event / degraded notes, which `PortfolioView` already
renders as a card overlay keyed to the rendered run.
The change is therefore mostly a **removal**: stop force-including from the
sweep and the deterministic legs, keep the sweep for the badge overlay and the
carried verdicts' condition eval-state chaining.

## Build inventory

- **Backend** (`src-tauri/src/portfolio/job.rs`): the selective work-list is the
  selection only; the deterministic force-include legs and the sweep-driven
  force block are removed; the carried tail is still swept (for the eval-state
  overlay and the persisted badge state); a carried directional verdict is marked
  `side_reversed` when it now sits on a net-short position — a directional verdict
  is only ever authored long (net-short / net-zero holdings are not-rated), so the
  marker compares that invariant authoring side against the current side rather
  than tracking per-run flips (robust across a flip through an exactly-zero net); a
  held position with no prior verdict is left not analyzed.
- **Domain** (`src-tauri/src/portfolio/mod.rs`): `HoldingVerdict.side_reversed`
  (a `#[serde(default)]` bool); the `whole_book_era_version` helper is retained
  (its second consumer, the action-prompt history label in `pipeline.rs`, still
  reads it) with its migration-consumer note removed.
- **Frontend** (`src/components/PortfolioView.vue`, `src/types.ts`,
  `src/App.vue`): not-analyzed placeholder cards derived from
  holdings-minus-verdicts and made selectable; a "Side reversed" badge on the
  carried card; stale copy about force-including corrected.
- **Tests**: the five force-include tests inverted to carry-and-badge, the
  pre-`v9` and side-reversal tests inverted, a not-analyzed test added; the
  `PortfolioView` spec covers the placeholder, its selectability, and the badge.

## Verification

`cd src-tauri && cargo test && cargo clippy --all-targets --all-features` (1037
lib tests pass, clippy clean); `npm run build && npm test` (241 component + 46
pure tests pass).

## Docs

The `docs/` corpus and the logic-flow doc are swept to the ruling:
`portfolio-analysis.md` §Triggering (the single-home) and its downstream
force-include references, the held-name-refresh-lane retirement across
`portfolio-analysis.md` / `portfolio-workflow.md` / `configuration.md` /
`storage.md` / `data-sources.md`, `interface.md`'s card states, and the
logic-flow §Work-list section.
The dated verification records under `docs/verification/` are left as
point-in-time history.

## Still open (Metis state)

`.metis/BUILD.md` (§What remains item 4 lists the held-name research refresh
lane as a queued depth slice; §Standing constraints / the built-slice notes)
and `.metis/INDEX.md` (the held-name-lane row) still describe the retired lane
and the force-include contract — to be updated as Metis state, since those
files are outside the implement-task write scope.
