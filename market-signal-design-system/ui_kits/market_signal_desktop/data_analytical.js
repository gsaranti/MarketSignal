// data_analytical.js — sample data for the analytical register
// (Portfolio Analysis + Trade Opportunities). Voice per the brief:
// declarative, specific, willing to name uncertainty. No emoji.

/* ---- Shared history sidebar content (per-feature run lists) ---- */
const PORTFOLIO_RUNS = [
  { id: "pf-0412", label: "Full book · 23 holdings", date: "Apr 12 · 09:14", read: "rated 19", current: true },
  { id: "pf-0405", label: "Full book · 23 holdings", date: "Apr 05 · 09:02", read: "rated 19" },
  { id: "pf-0329", label: "Full book · 22 holdings", date: "Mar 29 · 08:51", read: "rated 18" },
  { id: "pf-0322", label: "Energy sleeve only",      date: "Mar 22 · 18:30", read: "rated 4"  },
  { id: "pf-0315", label: "Full book · 22 holdings", date: "Mar 15 · 09:08", read: "rated 18" },
  { id: "pf-0308", label: "Full book · 21 holdings", date: "Mar 08 · 09:00", read: "rated 17" },
];

const TO_RUNS = [
  { id: "to-0412", label: "Full matrix · 3 × 3",      date: "Apr 12 · 10:40", read: "18 ideas", current: true },
  { id: "to-0405", label: "Full matrix · 3 × 3",      date: "Apr 05 · 10:31", read: "15 ideas" },
  { id: "to-0329", label: "Full matrix · 3 × 3",      date: "Mar 29 · 10:22", read: "11 ideas" },
  { id: "to-0322", label: "High-risk tier only",      date: "Mar 22 · 19:05", read: "6 ideas"  },
  { id: "to-0315", label: "Full matrix · 3 × 3",      date: "Mar 15 · 10:18", read: "9 ideas"  },
];

/* ---- Portfolio roll-up (whole book) ---- */
const BOOK = {
  value: "$1.84M",
  holdings: 23,
  rated: 19,
  notRated: 4,
  sinceRun: { dir: "up", val: "2.1%" },
  posture: "Defensive",
  cash: "Hold 6.8% dry. Fund energy adds by trimming the AI-infra cluster, not by raising new cash.",
  concentration: [
    { cluster: "AI infrastructure",  weight: "22.4%", names: 4, beta: "0.38", delta: { dir: "up",   val: "1.9" } },
    { cluster: "Mega-cap platform",  weight: "18.1%", names: 3, beta: "0.31", delta: { dir: "up",   val: "0.7" } },
    { cluster: "Energy · upstream",  weight: "11.6%", names: 2, beta: "0.14", delta: { dir: "down", val: "2.3" } },
    { cluster: "Rates-sensitive",    weight: "9.2%",  names: 3, beta: "0.09", delta: { dir: "flat", val: "0.0" } },
    { cluster: "Cash & equivalents", weight: "6.8%",  names: 1, beta: "—",    delta: { dir: "flat", val: "0.0" } },
  ],
  overlap: [
    { name: "Semiconductor capex", holdings: "ASML · NVDA · VTI (12% look-through)", note: "Single factor, three sleeves. The book is more concentrated than the position weights imply." },
    { name: "Long-duration rates",  holdings: "TLT · REIT sleeve",                    note: "Both express the same cut path. Sized as one bet, not two." },
  ],
  closed: [
    { ticker: "PXD", note: "Exited Apr 03 on the energy de-rate. Acknowledged here, not silently dropped." },
    { ticker: "SQ",  note: "Closed Mar 28 — thesis invalidated, not trimmed." },
  ],
  notRatedRisk: "AAPL Jun puts carry 2.1% of book at risk on a sharp drawdown. Unmodeled, but material to the roll-up.",
};

/* ---- Holdings (classified by asset type; class always shown) ---- */
const HOLDINGS = [
  {
    ticker: "ASML", name: "ASML Holding", sector: "Semiconductors", klass: "stock", state: "rated",
    grade: "A−", unrealized: { dir: "up", val: "34.6%" },
    thesis: "The single supplier of EUV lithography, and therefore a toll on every leading-edge node the AI build-out requires. The moat is not the order book; it is that no second source exists, and none is being built. We hold this for the decade, not the quarter.",
    sub: { quality: "A", valuation: "C", momentum: "B", risk: "B" },
    conviction: 4, eom: "$198", eoy: "$235", standalone: "Own here", health: "Net cash; through-cycle margins intact.",
    horizon: { short: "Range-bound into the July print", mid: "Re-rating resumes as 2nm orders land", long: "Structural toll compounds" },
    action: "Trim", targetWeight: "6.0–7.0%", weight: "8.4%", adj: "−1.4% · −$26k",
    rationale: "An A-grade business held at an oversized weight. We are trimming the position, not the thesis — concentration, not conviction, is the constraint.",
    scenarios: [
      { k: "Bear", p: "20%", t: "$150", note: "China export curbs deepen" },
      { k: "Base", p: "55%", t: "$210", note: "Orders normalize H2" },
      { k: "Bull", p: "25%", t: "$280", note: "2nm pull-forward" },
    ],
    changed: { intrinsic: "unchanged", action: "hold → trim", position: "unchanged" },
    curve: [26,24,25,20,21,16,14,15,9,6],
    triggers: { add: "Pullback below $170 with order book intact", trim: "Weight > 8% or valuation grade to D", sell: "A credible second EUV source emerges" },
  },
  {
    ticker: "XOM", name: "Exxon Mobil", sector: "Energy · integrated", klass: "stock", state: "rated",
    grade: "B−", unrealized: { dir: "down", val: "4.2%" },
    thesis: "The capital-discipline thesis from issue 140, held at the position level. Capex restraint is binding across the majors; the marginal barrel is no longer being underwritten. The four-week tape is not validating the timing — we hold the thesis, we no longer hold the timing.",
    sub: { quality: "B", valuation: "B", momentum: "D", risk: "C" },
    conviction: 3, eom: "$112", eoy: "$128", standalone: "Own here", health: "Free-cash-flow positive at $70 WTI.",
    horizon: { short: "Soft; crude range-bound", mid: "Re-rating if discipline holds", long: "Structural under-supply" },
    action: "Add", targetWeight: "5.0–6.0%", weight: "4.1%", adj: "+1.2% · +$22k",
    rationale: "Below target weight and the structural case is intact. Fund the add by trimming ASML — same dollar, better risk-adjusted entry.",
    scenarios: [
      { k: "Bear", p: "30%", t: "$92",  note: "Demand destruction" },
      { k: "Base", p: "50%", t: "$118", note: "Discipline holds" },
      { k: "Bull", p: "20%", t: "$140", note: "Supply shock" },
    ],
    changed: { intrinsic: "momentum B → D", action: "hold → add", position: "unchanged" },
    deadMoney: "Forward case clears the hurdle by 140bps — not flagged dead, but thin.",
    curve: [12,13,11,12,10,11,9,10,9,8],
    triggers: { add: "WTI two weekly closes below $70 with thesis intact", trim: "Capex discipline breaks at two majors", sell: "Sustained demand inflection" },
  },
  {
    ticker: "NVDA", name: "NVIDIA", sector: "Semiconductors", klass: "stock", state: "rated",
    grade: "A", unrealized: { dir: "up", val: "112.8%" },
    thesis: "The compute layer of the AI build-out. The question is no longer demand; it is whether the current margin structure is a peak or a plateau. We treat it as a plateau and size for the drawdown we cannot rule out.",
    sub: { quality: "A", valuation: "D", momentum: "A", risk: "C" },
    conviction: 4, eom: "$920", eoy: "$1,080", standalone: "Own here", health: "Pristine; the risk is multiple, not model.",
    horizon: { short: "Momentum intact", mid: "Margin normalization watched", long: "Compute toll compounds" },
    action: "Hold", targetWeight: "7.0–9.0%", weight: "8.9%", adj: "0.0% · in band",
    rationale: "At target weight with an A composite. No action — the valuation grade is the only thing staying our hand from adding.",
    scenarios: [
      { k: "Bear", p: "25%", t: "$640", note: "Margin re-rate" },
      { k: "Base", p: "50%", t: "$980", note: "Plateau holds" },
      { k: "Bull", p: "25%", t: "$1,300", note: "Inference TAM expands" },
    ],
    changed: { intrinsic: "unchanged", action: "unchanged", position: "unchanged" },
    curve: [70,78,82,90,96,104,98,106,114,120],
    triggers: { add: "Valuation grade recovers to C on a drawdown", trim: "Weight > 9% or momentum breaks", sell: "Margin structure confirms peak" },
  },
  {
    ticker: "VTI", name: "Vanguard Total Market", sector: "US equity · broad", klass: "etf", state: "rated-reduced",
    grade: "B", unrealized: { dir: "up", val: "9.1%" },
    thesis: "The book's beta anchor. Graded on exposure, valuation, and house-view — there is no company quality to score. Held as ballast, not as a call.",
    sub: { exposure: "B", valuation: "C", houseView: "B" },
    conviction: 3, eom: "—", eoy: "—", standalone: "Own here", health: "Diversified; the valuation read is index-level.",
    action: "Hold", targetWeight: "14.0–18.0%", weight: "16.2%", adj: "0.0% · in band",
    rationale: "Ballast at target weight. The reduced card is legitimate — an index fund has no company-quality score to compute, and that absence is shown, not faked.",
    changed: { intrinsic: "unchanged", action: "unchanged", position: "unchanged" },
  },
  {
    ticker: "AAPL 6/21 P", name: "AAPL Jun 21 $180 put", sector: "Options · hedge", klass: "option", state: "not-rated",
    reason: "Options are not modeled by the grading engine. Shown for completeness; its 2.1%-of-book tail risk is carried into the roll-up.",
    weight: "0.4%",
  },
  {
    ticker: "USD Cash", name: "Cash & sweep", sector: "Cash", klass: "cash", state: "not-rated",
    reason: "Cash is not graded. Tracked as deployable dry powder in the construction panel.",
    weight: "6.8%",
  },
  {
    ticker: "RXRX", name: "Recursion Pharma", sector: "Biotech · AI-enabled", klass: "stock", state: "insufficient",
    reason: "Insufficient evidence to grade. The model abstains rather than issue a low grade on a name it cannot underwrite — this is an explicit abstention, not an F.",
    weight: "1.1%",
  },
];

/* ---- Trade Opportunities · 3 × 3 risk × horizon matrix ---- */
// Each opportunity is keyed into a cell. Empty cells are honest.
const OPP = {
  CEG: {
    ticker: "CEG", archetype: "ai-infra", mode: "continuation", status: "still-valid",
    thesis: "Nuclear baseload is the only dispatchable power that clears the AI data-center load curve. Constellation owns the fleet; the PPAs are being signed now.",
    metric: { label: "Contracted TWh (fwd 24m)", val: "184", trend: "up" },
    catalyst: "Two hyperscaler PPAs expected before the July print.",
    conviction: 4, narrative: "Reality ahead of narrative — the contracts are signed, the multiple has not caught up.",
    bear: "Regulated-rate pushback caps the PPA premium.",
    falsifiers: "A PPA repriced below $80/MWh; a fleet outage > 30 days.",
    entry: "Scale in below $190; full size on a power-price pullback.",
    flags: "Concentration: single counterparty class (hyperscalers).",
    since: { return: { dir: "up", val: "31.4%" }, vsSector: { dir: "up", val: "18.2%" }, drawdown: "−9.1%", continuation: "intact", windows: "1m · 3m", curve: [10,12,11,15,18,17,22,26,24,29] },
  },
  VRT: {
    ticker: "VRT", archetype: "ai-infra", mode: "continuation", status: "still-valid",
    thesis: "Thermal and power management is the bottleneck inside the rack. Vertiv sells the picks for the liquid-cooling transition.",
    metric: { label: "Backlog ($B)", val: "7.4", trend: "up" },
    catalyst: "Liquid-cooling attach rate inflecting with the GB200 ramp.",
    conviction: 4, narrative: "Narrative and reality converging; the re-rate is mid-cycle.",
    bear: "Hyperscaler capex digestion pauses the order flow.",
    falsifiers: "Two quarters of flat backlog; attach-rate guidance cut.",
    entry: "Wait for a capex-scare pullback; the entry matters more than the thesis here.",
    flags: "Forensic: aggressive backlog recognition — watch the cash conversion.",
    since: { return: { dir: "up", val: "12.7%" }, vsSector: { dir: "down", val: "1.4%" }, drawdown: "−14.2%", continuation: "intact", windows: "1m", curve: [20,22,19,24,21,26,23,28,25,27] },
  },
  FSLR: {
    ticker: "FSLR", archetype: "secular-compounder", mode: "early", status: "new",
    thesis: "Domestic-content solar is being re-shored by policy and demand. First Solar's thin-film avoids the polysilicon supply chain entirely.",
    metric: { label: "Booked GW (2026+)", val: "61", trend: "up" },
    catalyst: "IRA domestic-content adder finalized in the next ruling.",
    conviction: 3, narrative: "Narrative lagging reality — the bookings are de-risked through 2026.",
    bear: "Policy reversal post-election guts the adder.",
    falsifiers: "Adder struck down; a major booking cancellation.",
    entry: "Starter here; add on the policy ruling.",
    flags: "Event-impact name — see technology read below.",
    tech: "Thin-film efficiency now within 2pts of crystalline; the cost-per-watt gap is the moat.",
    since: null, // debut — no track record yet
  },
  EQT: {
    ticker: "EQT", archetype: "commodity-cyclical", mode: "early", status: "still-valid",
    thesis: "Natural gas is the bridge fuel the AI power build-out cannot avoid. EQT is the lowest-cost Appalachian producer.",
    metric: { label: "Free-cash breakeven", val: "$2.10", trend: "down" },
    catalyst: "LNG export capacity steps up through 2025.",
    conviction: 3, narrative: "Reality ahead — the breakeven keeps falling while the multiple sits at trough.",
    bear: "A warm winter floods storage and caps the strip.",
    falsifiers: "Breakeven rises two quarters; LNG schedule slips.",
    entry: "Scale on gas-price weakness, not strength.",
    flags: "Cyclical — size for the drawdown.",
    since: { return: { dir: "down", val: "6.3%" }, vsSector: { dir: "down", val: "3.1%" }, drawdown: "−18.4%", continuation: "watch", windows: "1m · 3m", curve: [18,16,17,14,15,13,14,12,13,11] },
  },
  ANET: {
    ticker: "ANET", archetype: "quality-compounder", mode: "continuation", status: "still-valid",
    thesis: "The networking layer of the AI cluster. Arista's merchant-silicon model wins as scale-out fabric standardizes.",
    metric: { label: "AI cluster design wins", val: "11", trend: "up" },
    catalyst: "400G→800G transition with the next hyperscaler refresh.",
    conviction: 4, narrative: "Fairly priced for the base case; the optionality is unpriced.",
    bear: "White-box switching commoditizes the merchant model.",
    falsifiers: "Design-win count flattens; gross margin < 60%.",
    entry: "Quality at a fair price — accumulate, do not chase.",
    flags: "None material.",
    since: { return: { dir: "up", val: "8.9%" }, vsSector: { dir: "up", val: "2.1%" }, drawdown: "−7.8%", continuation: "intact", windows: "1m · 3m · 6m", curve: [14,15,14,16,17,16,18,19,18,20] },
  },
  IONQ: {
    ticker: "IONQ", archetype: "disruptor", mode: "early", status: "played-out",
    thesis: "Trapped-ion quantum has a coherence-time edge. The commercial timeline, however, keeps slipping past the window we underwrite.",
    metric: { label: "Algorithmic qubits", val: "36", trend: "flat" },
    catalyst: "A roadmap milestone — repeatedly deferred.",
    conviction: 2, narrative: "Narrative far ahead of reality. We are flagging this as played-out, not adding.",
    bear: "Commercial revenue stays a rounding error through the horizon.",
    falsifiers: "A named enterprise contract at scale would re-open it.",
    entry: "No entry at present size; held on the watchlist only.",
    flags: "Forensic: revenue quality thin; insider selling.",
    since: { return: { dir: "down", val: "22.6%" }, vsSector: { dir: "down", val: "29.4%" }, drawdown: "−41.0%", continuation: "broken", windows: "1m · 3m · 6m · 12m", curve: [30,28,31,24,20,22,16,14,12,9] },
  },
  WM: {
    ticker: "WM", archetype: "quality-compounder", mode: "continuation", status: "still-valid",
    thesis: "The toll road of waste. Landfill scarcity is a regulated moat; price escalators run ahead of cost inflation every year, regardless of the cycle.",
    metric: { label: "Core price / yield", val: "+6.1%", trend: "up" },
    catalyst: "Renewable-natural-gas plants step into the numbers through 2025.",
    conviction: 4, narrative: "Fairly priced for a compounder; the RNG optionality is the unpriced leg.",
    bear: "A recession softens volume faster than price can offset.",
    falsifiers: "Core price below CPI two quarters; an RNG plant impairment.",
    entry: "Boring on purpose. Accumulate on any market-wide drawdown.",
    flags: "None material. The lowest-beta idea in the matrix.",
    since: { return: { dir: "up", val: "4.2%" }, vsSector: { dir: "up", val: "0.9%" }, drawdown: "−3.4%", continuation: "intact", windows: "1m · 3m", curve: [10,10,11,11,12,12,13,13,14,15] },
  },
};

// Cell layout: rows = risk (high/medium/low), cols = horizon (short/mid/long)
const MATRIX = {
  high:   { short: ["IONQ"],        mid: ["FSLR", "EQT"], long: ["CEG"] },
  medium: { short: ["VRT"],         mid: ["ANET"],        long: [] },
  low:    { short: [],              mid: ["WM"],          long: [] },
};
// (Some cells deliberately empty — "nothing qualified," honest, not an error.)

const CALIBRATION = {
  shadow: true,
  picks: 47, matured: 19,
  hitRate: "58%", avgReturn: { dir: "up", val: "6.4%" },
  vsBench: { dir: "up", val: "2.1%" },
  failures: "Two-thirds of misses were timing, not thesis — the metric inflected later than the window allowed.",
};

window.MS_DATA = Object.assign(window.MS_DATA || {}, {
  PORTFOLIO_RUNS, TO_RUNS, BOOK, HOLDINGS, OPP, MATRIX, CALIBRATION,
});
