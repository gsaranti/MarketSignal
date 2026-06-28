# Market Signal — Design System Iteration Brief

## 1. Why we're iterating

Market Signal was designed as a **single weekly market report** — a reading-shaped,
prose-first research bulletin. The product has since bifurcated. It now also ships
two **local analysis features**:

- **Portfolio Analysis** — grades the user's holdings (A–F + sub-scores),
  conviction, price targets, a standing per-holding thesis, and whole-book roll-up.
- **Trade Opportunities** — discovers new ideas across a **3×3 risk×horizon
  matrix**, with leading metrics, a since-flagged performance read, and a watchlist.

These are **structured numeric data**, not prose. The current system was written
to *reject* the financial-data look — but it rejected the wrong half. We want to
move toward the **instrument-grade** financial gestalt (Bloomberg terminal, FT
market pages, a research desk's working tools) and stay as far as ever from the
**consumer-trading** gestalt (Robinhood: gamified, celebratory, saturated-green
P&L). The current rejects-list is almost entirely aimed at the latter, and that
aim is unchanged.

**Goal:** ~50% reading product / 50% technical analytical product, achieved by a
**register split**, not by washing every surface toward "technical."

**Scope — read this first.** This is a **look-and-feel iteration, not a re-layout.**
The existing **page structure, component layout, and information architecture stay
exactly as they are** — across the whole app, and the **Market Signal Report
surfaces especially**. We are changing *how components look* (color, type, density,
new component treatments), **not where they go, how pages are organized, or the
navigation model**. Do not propose moved panels, new page layouts, restructured
report sections, or a re-flowed sidebar/chrome. The component *inventory* may grow
(new analytical components in §5), but existing surfaces are restyled in place.

**Watchword for every decision:** *instrument-grade precision, never
consumer-trading excitement.* If a change makes the app feel like a professional's
tool, take it. If it makes it feel exciting/gamified, revert.

---

## 2. The register model (the core idea)

The app gets **two visual registers** plus shared chrome:

- **Reading register — the Market Signal Report. UNCHANGED.** Serif body, single
  64–72ch column, generous 8px baseline rhythm, strictly monochrome. This is the
  brand's strongest asset; it does not move.
- **Analytical register — Portfolio Analysis & Trade Opportunities. NEW, technical.**
  Denser, data-forward, mono numerics, hairline data grids, key-figure strips,
  controlled data cards, and a desaturated directional/grade palette. This is the
  "50% technical."
- **Shared chrome — sidebar, run tracker, persistent warning band, settings.**
  Stays in its current register but picks up the technical numeric treatment
  (mono tabular figures, tracked-caps labels) so the two registers read as one app.

The "50/50" is the product's **center of gravity** shifting because half the
product is now analytical — *not* a blend applied uniformly to every pixel.

---

## 3. What stays BINDING (do not change)

These remain absolute across **all** registers, including the new analytical one:

- **Page structure, layout & information architecture do not change.** Restyle in
  place — no moved panels, no re-flowed pages, no restructured report sections, no
  new navigation model. (Repeated from Scope because it's binding.)
- **Flat with hairlines.** No shadows, no inner shadows, no glass/frost/blur, no
  gradients, no z-axis elevation. A card is a hairline rectangle — full stop.
- **Radii ≤ 2px. Pills/capsules prohibited.** Any radius ≥ 4px is a violation.
- **Warm tones.** Warm ink on warm paper (light) / warm graphite (dark). **No
  pitch-black terminal background, no neon glow.** The existing graphite dark mode
  *is* our technical dark — make it denser, not darker-and-neon.
- **The report stays monochrome and serif.** The new palette never touches it.
- **No emoji, anywhere.**
- **No celebratory/feed motion.** No confetti, toast theatrics, number-counter
  spin-ups, skeleton shimmer, scroll-reveals, parallax. Motion only confirms a
  state change (120ms ease-out; navigation is a hard cut).
- **On-demand, not real-time.** No live-ticking tickers or faked liveness — the
  app generates on demand. Sparklines/figures are data-honest snapshots, never
  animated "live" theater.
- **No "Powered by AI" badges. Left-aligned hierarchy, no marketing hero blocks.**
- **Anti-references still binding:** Robinhood (consumer-trading), Linear
  (elevated-SaaS gradient/glow/command-palette-as-personality), Notion/Substack
  (soft-startup emoji/illustration). Drift toward any → revert.

---

## 4. What we're deliberately RELAXING (decisions, not drift)

Four decisions were made deliberately. Record each as a documented extension in
`SKILL.md` / `README.md` (don't silently resolve — note what changed and why).

### 4a. A new desaturated **analytical palette** — the spine of this iteration
The system has been strictly monochrome + one oxblood accent. We add a **small,
desaturated palette confined to the analytical register**:

- A **muted green** + the **existing oxblood** (`--accent #6E2230` light /
  `--accent-dk #B0596A` dark) + a **neutral mid** tone.
- **Unify it:** this *same* palette serves **all three** of the uses below —
  do not invent separate color sets for direction vs. grades.
- **Claude Design to pick exact hex values**, validated for **WCAG AA** as both
  text and as small fills, in **both light and dark** themes. Desaturated to match
  oxblood's restraint — it must never read as trading-app green.
- **Analytical register only.** Never in the report, never in generic chrome.

### 4b. Directional treatment (up / down / flat) in the analytical register
Today: sign + position + weight + neutral chevron, no color. Now: the same, **plus
the desaturated directional pair** (muted-green up / oxblood down / neutral flat).
Still **no saturated red/green**. Used for P&L, deltas, and change figures in
Portfolio & TO.

### 4c. Richer **controlled** data cards (the relaxation of the "dashboard widget" ban)
The "big-number + sparkline + percent-pill" card was banned. We reopen it
**narrowly** for the analytical register:

- A flat **hairline** card may now carry an **inline restrained sparkline/
  mini-chart** + a **delta** figure + key metrics.
- Sparkline = **journal-figure restraint** (single ink weight, **one accent series
  max**, the same ethos as the report's `chart` blocks). Data-honest, on-demand.
- Delta in the directional pair; all figures **mono tabular**.
- **Still** no shadow, no pill, no radius > 2px, no celebratory framing.
- Analytical register only.

### 4d. Grade scale (A–F) as a **desaturated tonal scale**
Letter grades and conviction map across the **unified palette** (muted-green → neutral
→ oxblood). Define a **discrete, small step set** with AA-validated text/background
pairings. A grade "chip" is a hairline/flat treatment, never a glossy badge.

---

## 5. New components to design (the concrete asks)

For the **analytical register** (Portfolio + Trade Opportunities):

1. **Key-figure strip** — a flat, hairline-delimited row of label-over-value pairs;
   values in mono tabular. The on-brand at-a-glance scan unit.
2. **Holding card (controlled-rich)** — grade chip, conviction, price target,
   the **standing-thesis anchor** (the continuity-validated thesis is the card's
   anchor text — must handle a long thesis with graceful overflow), optional
   restrained sparkline, directional delta.
3. **Dense data grid** — for the **TO 3×3 risk×horizon matrix** and the genuinely
   tabular roll-up breakdowns (concentration, sector/factor exposure, overlap
   clusters): tracked-caps column headers (11px), mono tabular body, right-aligned
   numerics, hairline row rules, the **since-flagged read inline**, sortable-column
   affordances where a surface genuinely sorts. The most naturally "terminal" surface
   — lean in. **Note:** the Portfolio **holdings view is cards, not a table** (the
   documented IA), so this grid is for the matrix and tabular roll-up data — *not* a
   sortable holdings table.
4. **Grade chip + grade scale** — the discrete tonal steps from 4d.
5. **Directional value token** — the up/down/flat treatment from 4b (sign + weight
   + chevron + desaturated hue).
6. **Methodology affordance** — sub-scores/targets expose their methodology; a
   restrained way to reveal "how this number was computed" (the engine is
   deterministic and methodology is a product principle).
7. *(Optional)* **Conviction meter** — a flat, hairline scale indicator.

---

## 6. Type & density adjustments

- **Promote IBM Plex Mono to first-class in the analytical register.** Today it's
  "optional, dense numeric tables only." Make it the face for **all** metrics,
  grades, deltas, targets, and **tickers** in Portfolio/TO. This single move does
  more for the terminal feel than anything else; tickers in mono especially read
  instrument-grade.
- **Public Sans** carries analytical labels and the tracked-caps (11px, +0.05em)
  column/section headers.
- **Source Serif 4** stays the report's — and only the report's — body face.
- **Density:** the analytical register is **denser than the report** — tight row
  rhythm on the 4px chrome step, hairline rules between rows (not padding). The
  report keeps its 8px baseline and generous measure.

---

## 7. What to hand back

- Updated **`colors_and_type.css`** — new unified analytical palette tokens
  (directional pair + neutral mid) and the grade-scale steps, light + dark, with
  AA notes.
- New **preview cards** (`project/preview/`) for: the analytical palette, the
  key-figure strip, the controlled-rich holding card, the dense data grid, the
  grade chip/scale, the directional token.
- New / updated **UI-kit components** (`project/ui_kits/market_signal_desktop/`)
  for the Portfolio and Trade Opportunities surfaces.
- Updated **`SKILL.md`** and **`README.md`**: add an **"Analytical register"**
  section, and amend the **"What this system rejects on sight"** list to record
  the four deliberate relaxations (so they read as decisions, not drift).

---

## 8. Decisions already made (for reference)

| Decision | Choice |
| --- | --- |
| How to apply the technical direction | **Register split** (report unchanged; Portfolio/TO technical; chrome ties together) |
| Direction / P&L color | **Desaturated pair, analytical-register only** (report stays monochrome) |
| At-a-glance figure unit | **Richer controlled data cards** (flat + hairline; restrained sparkline allowed) |
| Grade (A–F) differentiation | **Desaturated tonal scale** (shares the unified palette) |

---

## Appendix — Page layouts for Portfolio & Trade Opportunities

**Design these two new pages** (they don't exist in the UI kit yet) — but design them
**to the IA below**, which is what the product already specifies. This is the
user-facing surface inventory only: what objects appear, their key fields, the visual
hierarchy, and the states. (The engine mechanics, data sources, and job control flow
that produce these fields are out of scope for the visual design.) Both pages live in
the **analytical register** (§2) and compose the new components from §5.

### A. Shared surfaces — same across the Report, Portfolio, and Trade Opportunities

These are existing surfaces (restyle in place, don't re-architect), and they are
**shared by all three features**:

- **The sidebar is the same everywhere — a deliberate new "shared history sidebar"
  pattern (a scoped extension, not pre-existing IA).** Today the sidebar is documented
  as *report* history only; this redesign reuses its **structure and treatment** — the
  dense, hairline-separated, descending-timestamp row list — for Portfolio and Trade
  Opportunities, swapping only the *content* per feature: recent **report issues** /
  recent **Portfolio runs** / recent **Trade Opportunities runs**. Same component, same
  density, same selected-item accent (the oxblood leading-edge rule). Do not design a
  different navigation pattern for the new pages. (This content-swap is a new product
  decision for this redesign — record it as an intentional extension, not as something
  the current IA already says.)
- **The run tracker is the same everywhere — and it is *not* a modal takeover.** All
  three jobs stream into the *one* run tracker (per-step progress, one row per request,
  streamed model output, a cancel control). It opens **in place of the main content
  pane** when a run starts, but the run lives in the **job-status footer**: while a run
  is in flight the footer offers **View progress**, and the user can **leave the tracker
  — e.g. open a report from the sidebar — while the run keeps going in the background**,
  with the footer returning them to it; after a run ends, its trace lingers as a
  reopenable **Latest run log** for the session. Design it as a leaveable / re-enterable
  view, never a locked overlay. Only the per-unit progress label differs: the report
  streams per-step, **Portfolio streams per-holding**, **Trade Opportunities streams
  per-cell**. Only one run happens at a time across the whole app.
- **The persistent warning band is the same** — plus two new categories for the local
  jobs: **local models unavailable** and **Schwab connection required / re-auth**
  (both block the local jobs). Same band treatment as today.

### B. Portfolio Analysis page

**Page shape.** A holdings-analysis surface in the analytical register: a two-step
action affordance, the list of analyzed holdings (as cards), and a whole-book roll-up
panel. Dense, mono numerics, the desaturated palette for grades/direction.

**Trigger controls.** Two explicit, user-controlled steps, in order: **Pull holdings**
→ **Run analysis** (holdings are fetched only on explicit action — never auto-synced).
Design both the pre-pull empty state and the pulled-but-not-yet-run state. Holdings can
also be **supplemented by a manual paste / CSV import** (symbols, quantities, cost
bases) — for positions Schwab doesn't report — but this is a **supplement, not a
substitute**: it **does not bypass the required Schwab connection**, which gates the job
regardless. Show the import affordance without ever implying it can stand in for
connecting Schwab.

**Holdings, classified by asset type** (the class is shown, never hidden):
- **Stocks** — full verdict card (below).
- **ETFs / funds** — a *reduced* verdict card (graded on exposure/valuation/house-view;
  no company-quality score — the card must read as legitimately reduced, not broken).
- **Options / fixed income / cash / unsupported** — **not-rated**, shown with a short
  reason, no grade. A *material* not-rated position still appears in the roll-up's risk
  read.

**The holding card** (the most important new component — controlled-rich, §4c). It is
**anchored by the standing thesis** — the "why we hold this view," rendered as the
card's lead text. Two deliberately-separate-but-linked blocks below it:

- *Intrinsic verdict* — **composite grade (A–F)** + four sub-scores (quality,
  valuation, momentum, risk); **conviction**; **horizon outlook** (separate short / mid
  / long reads); **price targets** (end-of-month + end-of-year, with methodology
  reveal-able); a **standalone action lean**; a concise financial-health read.
- *Portfolio action* — the **action** on a fixed ladder (**sell all → trim → hold →
  add → add aggressively**) + a **target weight range** + an estimated share/$
  adjustment + the **sizing rationale** (the whole-book reason).
- The two must read as **intentional when they differ** (e.g. *A-grade business /
  trim because oversized* should read deliberate, not contradictory — show the link).
- *Bear / base / bull monitor* — three compact scenarios, each with its price target
  and a rough probability lean.
- *What changed* — a line split into an **intrinsic** half and an **action** half,
  carrying the **position delta** (new / increased / decreased / unchanged since last
  run).
- A **dead-money / capital-efficiency** flag when the forward case doesn't clear the
  hurdle.
- Falsifiers and add/trim/sell **triggers** with a target-weight range (likely a
  reveal, not always-visible — judge density).

**Portfolio roll-up & construction panel** (whole-book): overall **risk posture**; a
**cash / deployment stance** ("trim X to fund Y"); the **concentration & exposure**
read; **overlap clusters**; **positions closed since last run** (exited names,
acknowledged, not silently dropped); and the risk contribution of material not-rated
positions.

**States to design (frontend-craft):** no-holdings / pre-pull; pulled-not-run;
running (→ run tracker); stale holdings; expired Schwab OAuth; partial results
(checkpoint/resume); a holding at **insufficient-evidence** (explicit abstention, not a
low grade); degraded-input flags; **and the known open one — a long standing thesis
must overflow gracefully** (the thesis anchors the card and can be lengthy).

### C. Trade Opportunities page

**Page shape.** The page is organized as a **3×3 matrix**: three **risk tiers**
(high / medium / low) × three **horizons** (short / mid / long). The user reads it as
**high- / medium- / low-risk sections, each holding short- / mid- / long-term ideas.**
Each cell holds **however many opportunities clear the gates, ranked by conviction —
or none.** Empty cells are **honest, not errors** — design the empty cell so it reads
as "nothing qualified," never as a failure or a thing to pad. A rich cell with many
ideas is equally valid — the layout must handle 0 → many per cell. This is the most
naturally "terminal" surface — lean into the dense data-grid treatment.

**The opportunity card** (analytical register, controlled-rich). Leads with its
**directional thesis**. Fields: **ticker**; **archetype** (one of five:
secular-compounder / ai-infra / commodity-cyclical / disruptor / quality-compounder);
**detection mode** (early / continuation); the **leading operating metric** + its trend
(the countable anchor — visually prominent, it's the thesis's spine); **catalyst** (why
now); **conviction**; **narrative-vs-reality** read; **bear case** (always present);
**key falsifiers**; **hypothesis lineage** (the world-change → mechanism → node →
metric chain, reveal-able); an optional **technology read** (only on event-impact
names); **entry consideration**; **risk / forensic flags**; and **status** (new /
still-valid / played-out / invalidated).

**Since-flagged performance** (carried-forward ideas only — **this is the natural home
for the controlled-rich sparkline, §4c**): the running **return since first surfaced**
(absolute and vs sector / market), **max drawdown**, the **leading-metric-continuation**
state, and a **running curve** (the restrained sparkline). As windows elapse, the
**1 / 3 / 6 / 12-month** labels attach here. A brand-new idea has no such read yet —
design that "debut, no track record yet" state.

**Calibration scorecard** — the job's honest track record on prior picks (matured-window
outcomes + failure modes). Early runs are **shadow / calibration** — surface that state
honestly (the numbers are shown but not yet steering the job).

**States to design (frontend-craft):** empty cells (honest); a fully empty matrix
(nothing qualified this run); running (→ run tracker); insufficient-evidence
abstentions held out of the matrix; degraded-input flags; the shadow/calibration
banner; and density at the **~720×480 window floor** (a 3×3 grid of multi-field cards
is the hardest responsive case in the app).
