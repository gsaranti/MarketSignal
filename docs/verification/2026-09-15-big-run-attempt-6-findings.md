# Big confirmation run — findings (2026-09-15, attempt 6, ended at 6 of 47)

Findings from the fourth launch of the single big confirmation run — the
`portfolio-v35` debut, from a store wiped to a clean debut on 2026-09-14, across
the same 47-position book.
This is attempt 6, launched 2026-09-15 19:04 PDT (2026-09-16T02:04Z) and
user-ended at 6 of 47 completed holdings — TSLA, PSX, SPMO, ARKF, DIA, PGNY, with
NFLX cut off after two of its seven research topics — by a cooperative
run-tracker cancel at 04:19:48Z, after 2 h 15 m, once the user judged that the
local model calls were not yet at an acceptable state and made that the priority
over a completed run.
Entries written while the run was in flight keep their "interim" labels and
holding counts; the six-holding totals are in §Metrics observed.
Attempts 1 and 2 failed in the since-removed construction stage; attempt 3
(2026-08-30) was cancelled at 2 of 47 on the keyless SearXNG rate-limit spill;
attempt 4 (2026-08-31) was cancelled at 7 of 47 once its findings were in;
attempt 5 (2026-09-01) was user-ended at 4 of 47 to free the machine for a trip.
The watch set it reads against is `big-run-watch-set.md`; the attempt-5 record it
carries forward from is `2026-09-01-big-run-attempt-5-findings.md`.

The goal of this attempt is a COMPLETED run (ruled 2026-09-15): attempts 3, 4 and 5
were all ended early for findings, and the rates, outcome learning and the run-2
baseline all need a completed run 1.
Two early-stop reasons only were ruled, both read on the first holdings and both the
user's call once surfaced — a wrong-looking unit-exclusion rate, and Qwen reasoning
still oscillating after v35.
Everything else is recorded here as a watch while the run continues.

It keeps the attempt-3/4/5 thematic finding numbers, so a reader can compare a
finding across runs — Finding 1 is the search backend, Finding 2 the ledger
`quant`, Finding 3 the action call, Finding 4 the 6d research-findings turn,
Finding 5 the 6d synthesis shape, Finding 6 the fund path — and each entry states
whether it reproduces, does not reproduce, or is resolved relative to attempt 5.
Findings numbered 7 and up are new to this run.
After the run ended the findings were grouped into two sections — those that need a
fix or an adjustment before the next attempt, and those that are notes with no
change proposed — with each finding's thematic number kept.
Where a finding rests on a single holding it is labelled as such and carries a
quantify-across-the-book action rather than a conclusion; a single holding is not
a rate.
Nothing is changed while the run is in flight — every fix is deferred to after the
run, so adoption can read the run's own measured rates rather than a one-holding
impression.

## Run configuration

Dev app (`npm run tauri dev`, dev-scoped store) with the production corpus
re-imported by the user just before launch — 30 reports (newest the 2026-09-16T01:57Z
"The Flip-Triggers Fire" report, generated in the prod app minutes earlier), 69
vector rows, 14 baseline snapshots — and the portfolio store at its clean debut
(`prior_run_id = None`, zero prior runs, checkpoints, episodes or quick-checks;
`web_source_state` present in the v6 shape with `failed_count` / `denied_count`,
zero rows at launch).
The reasoner is `qwen3.5:122b-a10b` on the M5 (Ollama v0.32.5 pinned,
`OLLAMA_FLASH_ATTENTION=1`, `OLLAMA_NUM_PARALLEL=1` confirmed on the serve log;
`ollama ps` shows the model at 100% GPU with `CONTEXT 131072` after the first call).
Web research runs SearXNG-only with the Serper.dev paid Google SERP as the floor,
re-rendered into the runtime settings at bring-up.
At the pre-run per-engine probe Serper answered with real organic URLs (9 results),
and `google`, `bing`, `google cse` and `reuters` each served, while `qwant` and
`duckduckgo` returned CAPTCHA and `mojeek` returned empty — the same volatile-block
pattern the prior attempts documented.
The session ran directly under Terminal.app (ancestry printed before launch), so the
Claude.app Desktop-folder deny recorded on 2026-09-15 did not apply; Vite served
`index.html` HTTP 200 and the Desktop grant held before and after the dev-app
launch.
The dev binary was confirmed fresh against the source (built 17:42 PDT, no source
file newer, `PROMPT_VERSION = "portfolio-v35"`).
Header stamps observed on the persisted checkpoint header: `portfolio-v35`,
`checkpoint-v9`, `evidence-floor-v5`, `grade-v2.3`, `targets-v6`, `pre-profit-v4`,
`quick-check-v4` — the debut set the watch set requires; `model_ids` names
`qwen3.5:122b-a10b` for both roles; `holdings.positions` carries 47 rows;
`work_list` is null (unit exclusions are evaluated per holding at the statement
seam, not up front).
The house view rendered the three newest report summaries, the 2026-09-16 import
first, so the re-import reached the run.
Run identity: progress id `5f629ffe-ee94-4723-9f99-94013468dcc8`, portfolio run_id
`8d6b0b88-c00a-4b93-9bf0-13c7b1542eac`, thought-log folder
`20260916-020416-5f629ffe`.
No `job_runs` row existed while the run was in flight (the table stayed at id 5
until the terminal row was written at the cancel), consistent with attempt 5.

## Stop rule 2 — Qwen oscillation under v35: synthesis fixed, action untouched, interpretation bimodal (interim, six holdings)

Read per the thought-log oscillation method: each holding file segmented on the
`==== call` fences, each call classified by its stage field, and the `wait` /
`actually` / `re-read` markers counted per class with their ±150-char cause bucket,
against the attempt-5 v34 baseline expressed per holding (four holdings: synthesis
~54k words / 381 markers, interpretation ~10k / ~30, action ~9k / 40, gathering
~9k / 0).

| Call class | v34 per holding, words / markers | TSLA | PSX | SPMO | ARKF | DIA | PGNY |
|---|---|---|---|---|---|---|---|
| Synthesis | ~13,500 / ~95 | 8,984 / 25 (7) | 14,015 / 40 (8) | 5,914 / 27 (4) | 6,950 / 30 (4) | 7,608 / 22 (4) | 13,744 / 56 (7) |
| Interpretation | ~2,500 / ~7 | 692 / 0 | 3,513 / 20 | 844 / 0 | 5,496 / 46 | 748 / 1 | 1,048 / 4 |
| Action | ~2,300 / 10 | 2,172 / 17 | 2,600 / 17 | 4,002 / 21 | 2,287 / 17 | 1,477 / 8 | 2,249 / 12 |
| Gathering | ~2,300 / 0 | 2,959 / 2 | 4,356 / 1 | 931 / 0 | 1,551 / 5 | 1,263 / 1 | 2,316 / 0 |

Synthesis is where the v35 bundle landed: markers per holding fell from ~95 to
25–56 on the stocks and 22–30 on the funds (PGNY the noisiest at 56, twenty of them
the seeds/citability question), and the attempt-5 tell — the synthesis
call resolving "JSON or Markdown?" before planning its content — is gone on all
six holdings; every synthesis call opens by naming the strict JSON object as its
output format.
The residual synthesis markers are mostly substantive evidence reasoning, with
three prompt soft spots left as prompt-clarity candidates for after the run: which
seed IDs "genuinely oriented this pass" (seeds/citability), the meaning of the
`topic_answered` boolean and the exact key set (schema/keys), and — on the funds —
a `rung` bucket that is really the fund-exposure-fit topic asking the model to judge
fit against a house view the prompt never shows (Finding 9).
Interpretation is bimodal: TSLA, SPMO, DIA and PGNY are clean top-down walks with at most four markers,
while PSX (20) and ARKF (46, 5m11s — the noisiest call of the run) spend their
markers on the response-shape template and two instruction sentences rather than on
the holding — see Finding 8.
Action did not move on the first four and eased on DIA and PGNY (8 and 12 markers,
three and five on the clause) — see Finding 3.
Read as a whole, the v35 bundle cut the synthesis churn by roughly two thirds and
left the action call untouched; what remains is concentrated on two prompts (the
action call's ENGINE SET clause, the interpretation call's shape template and two
instruction sentences) rather than spread through the reasoning, which is the
fixable shape.
Stop rule 2 was surfaced to the user on the TSLA read and the user ruled to let the
run continue; the per-class comparison keeps accumulating as holdings land.

## Stop rule 1 — Unit and overlay exclusion rate: none on the first six (interim)

TSLA, PSX and PGNY passed the statement seam as ordinary USD equities; SPMO, ARKF and DIA all
priced as `US equity fund` with `structural_flag = false`, so the overlay name
screen removed none of the three.
NIO, the book's one ADR, is ninth in the work order (after NFLX and META); the null-`reportedCurrency` rule reads when it lands.
Update this entry with the count of excluded holdings and each exclusion's typed
reason when they land.

## Findings that need a fix or an adjustment before the next attempt

Ordered by consequence: the ledger numerics can misfire a machine-evaluated
condition; the action and interpretation prompts are where the model's reasoning
is spent on the prompt instead of the holding; the fetch layer is the throughput
and coverage cost; the exposure-fit topic and the period drift are prompt gaps.
Every fix here is deferred to after the run, per the standing rule, and each carries
its quantify-across-the-book action for the next attempt.

### Finding 2 — Ledger `quant`: population holds, but margins (and on TSLA thresholds) are implausible on three of six holdings, one of them a false-breach (new defect)

Population does not reproduce the attempt-3 under-population: all six TSLA
conditions carry a `quant` object, and the two margin-series cores are sound
(`gross-margin below 0.16 ± 0.02` on both the falsifier and the sell trigger,
`net-margin below 0.037 ± 0.04`).
The three price-series cores are not:

| Statement | Persisted quant |
|---|---|
| NHTSA investigation results in operational constraints or recall affecting >20% of registered fleet (falsifier, `technology_class` true) | `price above 3560.21`, margin 3,944,589,657 |
| Stock trades above $485 without Robotaxi deployment confirmation by Q2-CY26 (trim trigger) | `price above 485.3`, margin 5,348,709,521 |
| Stock falls to ≤$145 on margin collapse without Robotaxi catalyst (add trigger) | `price below 13.9`, margin 70,429,518 |

The first is a qualitative regulatory condition that should have `quant` null (attempt
4's TSLA authored exactly that shape for its NHTSA falsifier); here it is bound to
`price` at ten times spot.
The second has a plausible threshold and a margin of billions of dollars, so it can
never breach.
The third has its threshold off by a factor of ten from its own statement.
Attempt 4's TSLA authored `price above 460.0 ± 0.25`, so this is model output on this
run rather than a schema change — the quant schema (`ledger_schema` in
`src-tauri/src/portfolio/mod.rs`, `margin` required as a number) is unchanged since
the ledger slice landed on 2026-08-03.
The 6g validator accepted all three: `parse_quant_core`
(`src-tauri/src/portfolio/pipeline.rs`) checks that the series resolves, the
comparator is below/above, and the numbers are finite, then clamps the margin
non-negative — there is no plausibility guard, and `ledger_audit.downgraded` is
empty on TSLA.
The interpretation reasoning stream never mentions the quant numbers, so whether the
model reasoned to them or the grammar-constrained decoder forced digit runs where
the model had no number in mind cannot be read from the trace.
Effect on run 2: the trim trigger is unevaluable in practice, the add trigger fires at
$13.90, and the regulatory falsifier is machine-evaluated against a price it will
never reach — a loss of ledger evaluability, never a wrong action, since the action
call reads the verdict rather than the triggers.
PSX does not reproduce it: both of its conditions carry sane cores (`net-margin
below 0.03 ± 0.005`, `price below 210 ± 5` against spot $264.93), so the defect is
one holding of two.
PSX's ledger is thin, though — two conditions (one falsifier, one sell trigger)
against TSLA's six, with no add or trim trigger on a holding the model itself rated
mid-bearish and trimmed — a debut-authorship-quality watch rather than a defect.
One hypothesis for TSLA's numbers, readable in PSX's interpretation trace: the
v35 response-shape template shows `"margin":1` and `"threshold":1` as sample
numbers, PSX's model paused on whether the placeholder implied a magnitude and then
chose "reasonable" values per series, and a model that skips that pause under
grammar-constrained decoding could emit an unanchored digit run; the trace cannot
confirm it, so it stays a hypothesis for the post-run read.
SPMO reproduces the margin half of it on a different series: both of its
return-volatility cores carry a threshold of ~0.026 (2.6% daily) with margins of
0.15 and 2.0 — six and seventy-eight times the threshold — so neither can ever
breach; ARKF's two price cores are sane (`above 55 ± 3`, `below 38 ± 0.15`).
So over four holdings the implausible-numerics defect is 2 of 4 (TSLA on price,
SPMO on return-volatility), always on the margin, and on TSLA also on the
threshold and the series choice.
SPMO also gave the 6g executability contract its first live downgrade: a P/E
falsifier on a fund was downgraded to qualitative with the typed reason "series
'pe-ratio' has no fund-path computation — the condition would be permanently
unevaluable on this holding", exactly as designed (downgrade-not-drop).
DIA's two price cores are sane (`above 575 ± 0.5`, `above 648 ± 0.15` against spot
~$522), so over five holdings the defect stands at 2 of 5; DIA's five-condition
ledger is also the first to lean on the house view — its add trigger is the
imported report's own "one-and-done" Fed question and its trim trigger a 2Y-yield
funding-stress level, both correctly left qualitative since neither is an engine
series.
PGNY adds a third kind, and the first that would fire falsely rather than never:
its two revenue-growth cores are authored as `below 3.0 ± 0.02` and `above 8.0 ±
0.15` on a series the prompt and the engine both label "(decimal)" — the statements
say 3% and 8%, so the thresholds are 100× off, and the "fails to meet 3%" falsifier
reads breached against any real growth print (PGNY's TTM is 0.12) on the first
quick-check sweep, where the engine crossing would confirm it and the tripped claim
would be honored.
Over six holdings the numerics defect is 3 of 6 — TSLA (price threshold, margins,
series choice), SPMO (volatility margins), PGNY (growth thresholds as percents) —
and the guard candidate widens to a decimal-series threshold above 1.0.
Quantify-across-the-book action: over every completed holding, count price-series
cores whose threshold sits outside a sane band of the authoring close, whose margin
exceeds the threshold, or whose statement is qualitative (no numeric level named);
read whether funds and thin-data names carry more of them.
Candidate follow-up after the run: an app-side plausibility guard on
price-denominated cores in `parse_quant_core` — threshold within a bounded multiple of
the authoring close and margin below the threshold — downgrading to qualitative with
a typed reason (downgrade-not-drop, matching the existing executability contract), and
a prompt line that a condition with no numeric level on an engine series keeps
`quant` null even when a price could be imagined for it.

### Finding 3 — Action call: clean output on six of six, friction on the ENGINE SET clause on six of six (reproduces)

TSLA's action call landed `trim`, inside the engine set `[sell-all, trim, hold]`,
`action_source = model-chosen`, with a one-sentence rationale that names the
investment reason and the tax caveat as the prompt requires, and no departure
annotation.
The trace is 2,172 words with 17 re-think markers — essentially attempt 5's v34
per-holding action read (~2,300 words, 10 markers), so the v35 bundle did not move
this call.
Nine of the seventeen markers sit on one clause: the model read "the ENGINE SET shown
is the engine arm's own restriction, given as evidence, not a bound on you" four
separate times, each time asking why the engine would restrict the ladder if adding
were viable, before concluding it may choose from the full ladder and then choosing
inside the set anyway.
Four more sit on the tax-aware profile (whether "tax-aware" argues against selling)
and two on capital efficiency reading `indeterminate`.
The deliberation is not wrong — trim is the defensible rung on a double-F, bearish
short/mid, near-doubled position — it is spent on prompt semantics rather than the
holding.
PSX reproduces it exactly: 2,600 words, 17 markers, nine on the same ENGINE SET
clause, five on the tax caveat, and it too landed `trim` inside its set
(`[sell-all, trim, hold, add]`, engine `hold`) with a compliant rationale and no
departure annotation.
PSX's trace adds one more prompt gap: the action prompt's verdict block states the
polarity of the risk sub-score only ("higher = more resilient"), so the model asked
whether an engine valuation of 97 meant cheap or expensive before deciding to lean on
the research fair-value line instead; the interpretation prompt states "0-100,
higher better on every axis", and the action prompt should carry the same words.
The funds make it four of four: SPMO 4,002 words / 21 markers (eight on the ENGINE
SET clause, seven on what a failing capital-efficiency hurdle obliges — "exit input"
read as a command versus evidence), ARKF 2,287 / 17 (six on the clause).
ARKF is the run's first `sell-all`, model-chosen inside its set, and its rationale
is correct on the facts — the position is down 13% and the hurdle fails, so
harvesting the loss is the stated reason — which is the reassuring half of this
finding: four holdings, four defensible rungs, four traces spent on the same
sentence.
DIA is the lightest action call so far — 1,477 words, 8 markers, three on the
clause — landing `hold` inside its set with the engine also at `hold`; the clause
still shows on five of five, but a hold-versus-hold call gives the model less to
argue with.
PGNY: 2,249 words, 12 markers, five on the clause; `sell-all` model-chosen inside
its set against an engine `hold`, on a position down 28% with lowered guidance, the
loss-harvest rationale again correct on the facts.
That makes the two-arm split a pattern rather than a PSX oddity: on PSX, ARKF and
PGNY the engine's outlook is bullish on every horizon (momentum 100 on all three,
valuation 88–97) while the model reads the fundamentals bearish and cuts; the
action call sided with the model each time, inside the set, so no departure was
stamped — the two-arm watch's read is that the engine's momentum-led outlook and
the model's quality-led read disagree on low-quality names, and the rung follows
the model.
Quantify-across-the-book action: per holding, the action-call marker count and the
share on the ENGINE SET clause, and whether any holding's rung lands outside the set
(a departure annotation) after that deliberation.
Candidate follow-up after the run: restate the clause once, declaratively, in the
order the model reads it — the full ladder is yours; the engine set is one input;
choose, then stop — and drop the departure-mechanics sentence the model keeps
re-reading.

### Finding 8 — Interpretation call: bimodal — two of six holdings spend their reasoning on the shape template and two instruction sentences (new)

TSLA's, SPMO's, DIA's and PGNY's interpretation calls are the cleanest of the run
(692, 844, 748 and 1,048 words, at most four markers); PSX's (3,513 words, 20 markers, 3m47s) and ARKF's (5,496
words, 46 markers, 5m11s — the noisiest call of the run) are not about Phillips 66
or ARK.
Twelve sit on the response shape: whether the template's `"margin":1` placeholder
implies a magnitude, whether `what_changed` and `what_changed_entries` are both
required or one supersedes the other, and what the `what_changed_entries` schema
holds on a debut.
Six sit on one instruction sentence — "price_target_rationale explains the engine's
twelve-month base target or its absence; your own targets belong in
model_price_targets" (`src-tauri/src/portfolio/mod.rs`, the interpretation system
prompt) — which the model read four times as contradictory ("explain the engine's
target, but the targets are mine?") before deciding to frame its own $240 base
against the engine's $319.
The template caveat ("Sample numbers, booleans and neutral outlooks are not
findings") did its job — PSX chose sane magnitudes — but the model still paid for
reading it.
ARKF's 46 markers land on the same three items (27 on schema/keys) plus two more:
which ledger series a fund may use at all — it read the series enum, noticed
`expense-ratio` and worried whether a fund ledger was allowed price cores — and the
"beginning with `{`" fencing question, which the synthesis prompt's shape fix
closed but the interpretation prompt still leaves open.
Quantify-across-the-book action: per holding, the interpretation marker count and the
share on template/instruction text versus evidence; read whether thin-ledger
holdings (PSX authored two conditions) correlate with template deliberation.
Candidate follow-up after the run: state the two `what_changed*` fields' relation
once, and rewrite the `price_target_rationale` sentence so its subject is the
model's own base target with the engine's as the comparison.

### Finding 1 — Search backend under the research loop: serving cleanly; fetch economics are the cost (interim)

Reproduces attempt 5's read on the search side and sharpens the fetch side.
Every TSLA search returned hits (34 of 35 searches ok, 12 hits each; one empty), so
the Serper floor is doing its job and the serve-rate watch is green on the first
holdings.
The cost sits in the fetch layer: TSLA spent 26 of the 40-attempt fetch ceiling, 17
of them failed (WSJ HTTP 401 ×3, Reuters 401 ×3, nhtsa.gov 403 ×3, seekingalpha 403
×2, assets-ir.tesla.com 403 ×2, nytimes 403, ir.tesla.com 403, electrive 403,
siliconreview 404), so 9 pages served across seven topics.
Nine of the seventeen failures are the same three URLs (the WSJ, Reuters and NHTSA
Cybercab pieces) re-fetched by later topics — competitive-position, catalysts-risks,
narrative-sentiment and forward-thematic each tried them again — because the loop
carries no per-holding memory of URLs that already failed this run; the
`web_source_state` per-host counts are the only failure memory, and they are not
handed to the model.
All seven TSLA topics reported degraded coverage in `gaps`, four of them also hit the
8-turn gathering cap, and the forward-thematic pass synthesized on zero fetched
pages (its two fetches both failed) — the synthesis reasoning shows the model asking
whether it may make any claim with no S-ids to cite, and correctly emitting a
seed-only, no-claim findings object.
PSX shows the same shape early: investor.phillips66.com 403 ×4 and a PDF refused as
an unsupported content type.
The per-domain `failed_count` / `denied_count` telemetry from the 2026-09-14 slice
is live and reads correctly — every 401/403 host carries `denied_count` equal to its
`failed_count`, and the 404 host carries a failure without a denial.
PSX repeats the shape at a larger scale: 48 searches all served; 31 fetch attempts,
11 served and 20 failed, of which investor.phillips66.com HTTP 403 accounts for
eleven — the model tried the issuer's investor-relations pages topic after topic, and
the loop let it, so more than half of PSX's fetch budget went to one refused host;
the remainder were Reuters 401 ×2, seekingalpha 403 ×2, two IR PDFs refused as an
unsupported content type, sec.gov 403, marketscreener 403 and a 404.
PSX also recorded three served pages truncated at the 12,000-character fetch cap
(`PAGE_TEXT_CAP_CHARS`) and the distillation source budget truncating three pages
with none omitted — the first live reads of both bounds.
PSX's research ran 1,349 seconds of its ~30-minute wall clock (TSLA 926 of ~20), so
the fetch failures are the run's throughput cost as much as its coverage cost.
The two funds fetched light and fast — SPMO 7 served / 4 failed over 26 searches
(`fetches_spent = 8`, 535 s), ARKF 4 served / 5 failed over 23 searches
(`fetches_spent = 9`, 623 s) — with the issuer's own site refused again
(ark-funds.com 403 ×2, ark-invest.com 403) and sec.gov returning HTTP 403 on both
PSX and ARKF, which reads as the web fetcher lacking the declared User-Agent SEC's
fair-access policy requires (the EDGAR API legs, which do declare one, served every
holding); a fetcher-side fix candidate rather than a research-loop one.
DIA fetched 6 served / 3 failed over 18 searches (`fetches_spent = 7`, 694 s;
etf.com 403 ×2, ir.theice.com 403) and recorded four served pages truncated at the
12,000-character cap — the cap now binds on every fund as well as on PSX.
PGNY — attempt 4's failed holding — completed, but at the run's worst fetch ratio:
7 served / 21 failed over 31 searches (`fetches_spent = 28`, 1,500 s of a ~30-minute
holding), and fifteen of the twenty-one failures are investors.progyny.com, the
issuer's own investor-relations site, attempted on every topic and never served.
Unlike the 401/403 hosts these fifteen carry no denial (`web_source_state`: failed
15, denied 0 — the watch set's "dead or flaky source" signature), and the cause is
invisible: the progress row renders the fetch error with `e.to_string()`, which is
the outer anyhow context "fetching <url>" alone, while the model's tool result gets
`{e:#}` with the source chain, so whether the host timed out, refused TLS, or
reset the connection cannot be read from the tracker or the store — a one-line
telemetry fix candidate (render the chain into the row's detail).
With phillips66 (11 refused), progyny (15 failed) and the three paywalled TSLA
seeds, the repeat-failed-host share is now the dominant fetch cost on every stock.
Quantify-across-the-book actions: the fetch-failure rate and the denied share per
host at book scale (the watch set's render-tier evidence), the repeat-failed-URL
share, the count of zero-page syntheses, and how often the 8-turn cap and the
40-attempt ceiling bind.
Candidate follow-up after the run: a per-holding failed-URL set that the fetch tool
consults (a second attempt returns the earlier failure without spending an attempt),
and a seed-orientation note that the model should not re-fetch a URL the ledger
already shows failed.

### Finding 9 — The fund-exposure-fit topic asks for a house-view fit the research prompt never shows (new, three holdings)

The fund research agenda's third topic is titled "Exposure fit against the house
view" and asks "How well does the exposure this fund supplies fit the current market
thesis?" (`src-tauri/src/portfolio/research.rs`), but the gathering and synthesis
prompts render no house-view content — the Market Signal house view reaches only
the interpretation prompt.
SPMO's synthesis trace says so in as many words ("standard procedure implies I should
know the house view from context not shown here? No…") and both funds' exposure-fit
syntheses carry the run's highest `rung`-bucket marker counts (SPMO 4 of 8, ARKF 7
of 15; DIA's is quieter at 0 of 4) as the model reasons about a thesis it has to infer from the fund's name.
The persisted findings still answer the exposure-profile half of the question, so
the loss is the fit half and the reasoning spent on it.
Quantify-across-the-book action: the exposure-fit synthesis marker count on every
fund, and whether its `topic_answered` lands true or false.
Candidate follow-up after the run: render the house view's posture / cycle / stance
line into that topic's orientation, or retitle the topic to the exposure profile
and leave the fit read to the interpretation call, which does see the house view.

### Finding 7 — Period labels drift a year under the model's cutoff reflex (new, two of three stocks)

TSLA's persisted `research.combined` opens "Tesla's Q3 2026 financial performance
showed a top-line beat ($28.01B vs $26.27B expected) but an adjusted EPS miss ($0.50
vs $0.54), with operating profit dropping 40% YoY to $1.624B".
Those are Tesla's Q3 2025 prints; Q3 2026 has not reported on the run date.
The gathering trace shows the cause: the model's own queries asked for "Q4 2025 or FY
2025" estimate revisions as the latest period, two gathering turns called the
September 2026 seed URLs "future-dated", and one synthesis call reasoned that a
"Q4 2025 transcript retrieved Sept 2026" implied "a specific dataset scenario".
The model treats 2025 as the current year and relabels the retrieved periods forward.
The engine's metrics are unaffected (they come from the FMP statement basis, TTM to
the newest filed quarter) and the ledger thresholds reference series rather than
periods, so the exposure is the prose: `combined`, the seed-layer summaries, and any
verdict narrative that quotes a period.
The gathering and synthesis prompts carry per-page retrieved-at stamps and evidence
vintages but no explicit current-date line.
PGNY shows the staleness half of the same reflex: its `combined` opens with "6.4
million lives across over 460 employer clients as of March 2024" and "$787M revenue
in 2022" beside "record FY25 results including $1.3B revenue", so two-year-old seed
facts are presented as the current state next to the genuinely latest fiscal year;
no label is wrong, but the model did not rank the retrieved dates.
Quantify-across-the-book action: per holding, grep `combined` and the seed summaries
for quarter and fiscal-year labels and check each against the source's own dateline;
count mislabels.
Candidate follow-up after the run: a single dated line at the top of the research and
interpretation prompts ("Today is 2026-09-16; the newest filed quarter in the packet
is …") so the model anchors period labels to the run date rather than its training
horizon.

## Notes and observations — no change proposed

Reads that confirm a contract, resolve a prior finding, or record a behavior for the
user to confirm or for the next attempt to count; nothing here asks for a code or
prompt change on its own.

### Finding 4 — 6d empty-body rate under Fix B: two retries over six holdings, both recovered (interim)

Does not reproduce at the attempt-4 ~70% rate.
TSLA, SPMO, DIA and PGNY fired no retry (PGNY being attempt 4's hard-failed
holding, now clean); `output_limited = false` on every prompt-usage row of
all six holdings, and every synthesis call ended `stop` with a body.
PSX fired one retry, typed and topic-labelled as the 2026-09-14 telemetry slice
intended: `holding-PSX research narrative-sentiment synthesis — content failed its
parse`, the body being a bare `{`.
The fenced trace shows the shape of the failure: the first attempt (call 107) thought
through a complete findings plan — claims, follow-up, seeded-by — ended its
thinking on a stray "Let's assemble.cw", and then generated 56 tokens in total, so
the visible body never got past its opening brace; the bounded re-issue (call 108)
produced the full object with 12 markers and the topic reconciled.
ARKF fired the other: `holding-ARKF research fund-expense-structure gathering —
daemon error status`, Ollama returning HTTP 500 with "XML syntax error on line 4:
element <parameter> closed by </function>" — the serve log shows `qwen35.go` "tool
call parsing failed", so the model emitted a malformed tool-call XML and Ollama's
Qwen parser rejected it before the app saw a body; the retry recovered it.
That is the same holding and the same cause as attempt 5's ARKF retry, and the
two-retry / two-cause / zero-double-failure pattern is attempt 5's exactly (there
PSX parse + ARKF daemon), so on eight holding-runs across two attempts the retry
rate is 2 in 4 holdings and the recovery rate 4 of 4.
So on six holdings the 6d empty-body event is 1 in 34 synthesis calls, it landed on
the thin-evidence narrative-sentiment topic again (attempt 5's PSX topic), and the
retry recovered it with no topic loss.
The pass-shape bounds bind routinely: twenty-four of thirty-three topics across the six
holdings hit the 8-turn gathering cap and are recorded as partial-coverage gaps with
a fresh synthesis each, which is the contract the watch set requires.
The run-wide retry rate per cause is a completed-run read.

### Finding 5 — 6d synthesis shape under v35: resolved on six of six (interim)

All six holdings persisted `unreconciled_topics = []` — seven topics on each stock, four on each fund, and the
synthesis reasoning shows the schema-shape fix working — each call names the strict
JSON object first and plans content second (the stop-rule-2 table above).
The attempt-5 PSX loss — a confused narrative-sentiment topic dropped whole — did
not recur even though PSX's narrative-sentiment synthesis was again the one call to
fail its parse (Finding 4): under v35 the retry recovered the topic instead of
losing it.
The watch set's correlation read therefore stands at six holdings: parse retry on
narrative-sentiment 1 of 6, topic loss 0 of 6.
Quantify-across-the-book action per the watch set: the `unreconciled_topics` count
across completed holdings, whether they cluster on narrative-sentiment and
disconfirming, and their correlation with topic-labelled parse retries.

### Finding 6 — Fund path: all three funds price under the 2026-09-15 rulings; ARKF moved from role/risk-only to priced (interim)

All three fund holdings so far resolved `priced` with `fund_class_label = "US equity fund"` and
`structural_flag = false`, so the intrinsic verdict's role/risk branch has not yet
been exercised on this run.
SPMO (S&P 500 momentum, expense 0.13%, 99% US, 53% technology) priced grade C,
`trim` (engine `trim`), hurdle `fails`, ~14 minutes wall clock (02:54:39Z to
03:08:35Z); ARKF (ARK fintech, expense 0.75%, 70% US, 44% technology) priced grade
C, `sell-all` (engine `trim`), hurdle `fails`, ~18 minutes; DIA (Dow 30, expense
0.16%, 100% US, 27% financials) priced grade C, `hold` (engine `hold`), hurdle
`indeterminate`, ~15 minutes, no retry — the cleanest fund of the three.
DIA carries the same "Cash & Others" sector-P/E gap and 100%-coverage pair as ARKF,
so that pair reads as the fund path's resting state for any fund holding a cash
bucket (2 of 3 funds), not an ARKF property.
ARKF is the change to read: on attempt 5 it resolved `role_risk_only` with `grade =
None`; here it prices, with the same "Cash & Others" sector-P/E gap carried as a
`degraded_input` beside a second line reading "composite P/E coverage 100% of fund
weight — the uncovered 0% is reported beside the valuation read, never averaged in".
That is consistent with the 2026-09-15 ruling (only overlay funds go
`role_risk_only`; ARKF is an active thematic ETF, not an overlay) and with the
coverage line now letting a non-sector cash bucket ride alongside a fully covered
composite rather than withholding the price, but it is a behavior change on a named
holding and is recorded for the user to confirm as intended.
Fund research reads light by design (8–9 fetches, 4 topics) and carried the run's
first `unknown seeded_by reference(s) dropped` gaps — six across the three funds —
plus, on ARKF's fund-exposure-fit topic, three claims dropped for an unresolved
source id and three distilled claims dropped for an unknown source URL or expired
cache vintage: the distillation-reconciliation misses the watch set expects, now
with counts.
The fund-classification split, the priced-versus-role/risk distribution, and the
overlay screen's fire rate are the across-book reads only a completed run gives.

### Smaller watches (six holdings, no action yet)

- The research `leading_indicator` reads direction `inflecting-down` on a value
  the source calls a record high (236 driver-assist crashes in July 2026);
  `driver_verified` is false and `confirms_driver_id` names a driver that is not in
  the ledger, so it stays gap-noted evidence as designed — read the direction field's
  accuracy across holdings.
- `audit.narrative` is null although the narrative-sentiment topic ran and
  synthesized; check whether the narrative-vs-reality read requires an input the
  topic did not produce, or whether null is its no-hype-signal resting state.
- `degraded_inputs` carries "listing-resolution guard unverified — account
  description carries no issuer name to cross-check" on TSLA; count how many Schwab
  descriptions lack an issuer name at book scale, since each is a holding the guard
  cannot protect.
- A CNBC page rendered as `tier 4 | kinds ["unregistered"]` in the evidence list, and
  the model noticed the mismatch; check the source registry for cnbc.com.
- `forward_assumption_resolution` rejected the $20B capex guidance as mapping to no
  recomputable driver (EPS / revenue) — by design, recorded here as the first live
  instance.
- Engine 12-month targets on TSLA: base $163, bear $62, bull $912 against spot
  $356.58 (`clamp_released = true`, `degenerate_scenarios = 1`); the model's own are
  $145 / $92 / $398; the 2.6× bull is the width the targets watch reads at book scale.
- PSX two-arm divergence: engine grade D, `hold`, short/mid/long all bullish
  (momentum 100, valuation 97) against the model's F, `trim`, mid bearish — the
  widest arm split so far, resolved by the action call toward the model arm with no
  departure annotation since `trim` sits in the engine set; the two-arm watch's
  first live disagreement.
- The listing-resolution guard is unverified on both holdings ("account description
  carries no issuer name to cross-check"), so this reads as a Schwab-description
  property rather than a per-holding gap; count it at book scale.
- `audit.narrative` is null on both holdings although both ran the narrative-sentiment
  topic.
- PSX's typed-channel validation rejected as designed: the forward assumption
  dropped because "the cited page never states the value (a range fact must carry
  its stated endpoints)", and the leading indicator dropped because its as-of "2024"
  is not ISO day or month precision — first live rejections on both channels.
- Every stock position arrives from Schwab with an empty description (TSLA, PSX,
  PGNY, NIO all `''`) while the ETFs carry names, so "listing-resolution guard
  unverified" is a property of the stock rows book-wide, not of TSLA and PSX; the
  guard cannot protect any stock on this account until the description carries an
  issuer name from another field.
- The 2026-08-24 unverified-driver watch fired live on SPMO and DIA: the leading
  indicator cited invented driver ids ("DRV_MOM_01", "price_weighting_concentration_risk")
  that are not ledger drivers, and each stayed gap-noted evidence with no cap
  suppression, as ruled — 2 of 3 funds so far.
- ARKF's typed channels rejected as designed too: forward assumption "the cited page
  never names the holding", leading indicator "the cited page never states the
  metric's value".
- Both funds' hurdles read `fails` and both fund actions cut (trim, sell-all); the
  hurdle-to-rung coupling is a read for the fund population.
- The interpretation call's streamed thinking reads condensed in places (one
  fragment "d triggers complete" mid-log), so the interpretation word count is a
  floor; the fence trailer's `generated` token count (1,365) is the reliable size.

## Metrics observed — final at cancellation, six holdings

The run was ended at six completed holdings, so these are observations across a
short sample, not rates.

- Search backend: Serper plus `google` / `bing` / `google cse` / `reuters` serving
  at the probe; across the six holdings 180 searches served with hits and 20 came
  back empty; no spillover is possible (SearXNG-only).
- Fetch: 44 served / 70 failed across the six holdings (39% served); the failures
  concentrate on issuer investor-relations sites (investors.progyny.com 15 with no
  denial, investor.phillips66.com 11 denied) and paywalled or bot-blocked outlets
  (seekingalpha 8, reuters 6, wsj 4, nhtsa.gov 3, sec.gov 3); `web_source_state`
  holds 52 host rows, 42 documents cached (Finding 1).
- 6d empty-body / retries: 2 retries over 6 holdings (PSX parse on
  narrative-sentiment synthesis, ARKF daemon error on a malformed tool call), both
  recovered, 0 hard failures — attempt 5's pattern exactly (Finding 4).
- Reconciliation: `unreconciled_topics` 0 of 6 (Finding 5).
- Ledger `quant`: populated on every holding; numerics implausible on 3 of 6
  (Finding 2); the 6g executability downgrade fired once, correctly (SPMO).
- Fund path: 3 of 3 funds priced, 0 role/risk; ARKF changed branch versus attempt 5
  (Finding 6).
- Unit and overlay exclusions: 0 of 6; NIO never reached (Stop rule 1 unread).
- Context pressure: longest prompt 12,673 tokens (a distill) against `num_ctx =
  131072`; `output_limited = false` on every prompt-usage row of every holding.
- Throughput: stocks 20 / 30 / 30 minutes, funds 14 / 18 / 15; six holdings in 128
  minutes (~21 each); research took 60–75% of each stock's wall clock.

Six-holding reasoning-stream totals against the v34 four-holding baseline, per
holding (words / re-think markers):

| Call class | v34 (attempt 5, 4 holdings) | v35 (attempt 6, 6 holdings) | Change in markers |
|---|---|---|---|
| Synthesis | ~13,500 / ~95 | 9,536 / 33 | −65% |
| Interpretation | ~2,500 / ~7.5 | 2,057 / 12 | +60%, bimodal (0–4 on four, 20 and 46 on two) |
| Action | ~2,300 / 10 | 2,465 / 15 | +50%, on one clause (40 of 92 markers) |
| Gathering | ~2,300 / 0 | 2,229 / 1.5 | unchanged |

Residual cause buckets across all six holdings' 363 markers: schema/keys 106,
seeds/citability 55, the ENGINE SET clause 42, "other" (substantive) 69, rung 29,
spot-price 25, capital-efficiency 19, tax 17, exact-URL 5, code-fences 4.

## Run ended (2026-09-15)

The run was user-ended at six of forty-seven holdings by a cooperative cancel from
the run tracker, which wrote the terminal `job_runs` row (id 6, `cancelled`, "run
cancelled by user", started 02:04:16Z, finished 04:19:48Z) — the first attempt to
leave one.
The reason was a priority call: the reasoning-stream reads above showed the local
model calls not yet at an acceptable state — the action call and the interpretation
call in particular — and the user chose to fix that before spending the remaining
~14 hours on a completed baseline.
The local infrastructure — the dev app, Ollama, SearXNG, OrbStack and the session's
caffeinate — was spun down; the SearXNG runtime settings file (gitignored) is left on
disk and is re-rendered at the next bring-up as the runbook requires.
The dev store keeps the checkpoint header, the six completed holdings and the 52-host
`web_source_state` for inspection rather than being re-wiped; the next attempt
re-wipes to a clean debut as the standing ruling requires.
The run's stderr log, the Ollama serve log, the segmentation helper and read-only
store extracts were copied to `~/Downloads/market-signal-attempt-6-logs/` (outside
the repo and the app data dir) beside the thought-log folder in the app's dev data
dir, so a second reviewer can read the same evidence.

What only a completed run can still close: the unit and overlay exclusion rate (NIO
never reached), the run-wide retry rate per cause, the `unreconciled_topics` rate,
the fetch-failure and denied-share rates under full query volume, the fund
classification split, the quick-check and data-health reads, and every
outcome-learning input.
