# Action — priced holding, first analysis

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v71`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: TSLA, a stock of the fixed evidence set (attempt 6, reconstructed), on its first analysis.
The action call is the investor profile's one entry point: the finished verdict, the holding's own evidence, the engine's supported set and the profile decide the rung and one rationale — never a comparison with other holdings.
POSITION is the one packet that sees the position's economics — the synthetic ten units, their cost basis and market value, the unrealized gain and the change since the last pull, new on a first analysis.
VERDICT carries the appendix's conviction and expected prices, each with the move it implies from the current price, then the thesis document verbatim; COMPUTED follows as one heading whose sub-blocks carry the rule's rung, the grade, the bands with the analyst's price beside each, and the capital-efficiency numbers.
The two variants below show that a tax-exempt profile changes no line and a tripled cost basis changes POSITION's lines alone.
This packet renders no article or research text, so it carries no stub.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6f |
| Stage label | `action TSLA` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | the JSON schema below as the `format` grammar; no tools |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 7212 chars — the messages and tools as serialized |

## System message

~~~~text
You are an equity analyst deciding the portfolio action for one holding in a portfolio review. You
will return action and rationale, as one JSON object. Part 1 of the message gives the inputs. Part 2
defines those outputs and gives the shape to return.
~~~~

## User message (6807 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
TSLA (name unavailable).
Price: $358.97 per share.
Date: 2026-09-16.

POSITION
The holding as the account carries it: the shares held, the total cost basis, the market value, the
unrealized gain or loss (the market value less the cost basis, and as a share of a positive cost
basis; not available where no basis is reported) and the change in the shares held since the last
pull — new where the last pull had none, else increased, decreased or unchanged, with the shares
held then and now.
Shares held: 10. Cost basis: $2,871.76. Market value: $3,589.70. Unrealized gain: $717.94 (+25.0% of
the cost basis). Change since the last pull: new.

VERDICT
An analyst's read of the holding's data and research: the conviction, the expected share price at
each horizon (USD, with the move each implies from the current price) and the thesis document.
Conviction: low. Expected share price: three-month none, twelve-month 145.00 (-59.6%), three-year
none.
Thesis document:
Thesis: Investment thesis rests on Robotaxi monetization within 12-24 months justifying premium
multiple, with safety scandal resolution and margin stabilization as near-term catalysts; core EV
business provides downside floor at ~$180/share (base case earnings power) but offers limited upside
without autonomous revenue validation.

Key drivers: what must improve — Robotaxi deployment with regulatory approval by Q1-CY26; gross
margins must re-expand toward mid-20s through cost reductions and volume leverage; FSD safety
metrics need to demonstrably improve vs human baseline. What must not break — Gross margin floor at
17%+ over next four quarters; NHTSA investigation remains non-actionable without vehicle
restrictions or recall; regulatory credits remain ≥30% of operating income until Robotaxi revenue
materializes.

Scenarios: bear (35%): Gross margins breach floor at ≤16%, NHTSA mandates vehicle
restrictions/recall, FSD crashes increase >25%, or Robotaxi commercialization delayed beyond CY27.;
base (40%): Robotaxi commercialization begins Q4-Q1 CY26 without NHTSA enforcement action; gross
margin stabilizes above 17%; regulatory credits decline to <35% of operating income over 8
quarters.; bull (25%): Robotaxi fleet achieves profitable unit economics by Q2-CY26 with regulatory
approval; gross margins re-expand to >24%; Energy/storage revenue doubles within 18 months.

Expected price: twelve months $145.00; no stated price at three months or three years. Conviction:
low.

Summary: Tesla exhibits structurally thin operating margins (3.68% net) heavily reliant on
regulatory credits, with gross margin contraction to 18.85%. Revenue growth at 11.75% YoY reflects
deceleration from historical hyper-growth rates. Balance sheet remains fortress-quality with minimal
leverage (D/E: 0.11x), but the valuation is exceptionally extended across all multiples—P/E of 369x,
P/S of 14x, and P/B over 16x assume material Robotaxi monetization within 12-24 months without
commercialized revenue to validate these expectations. My base twelve-month target of $145 derives
from a conservative auto peer multiple (P/S: ~6x on normalized revenue) plus Energy/storage
optionality at ~30% discount to standalone value. I reject the engine's base case ($163) because it
assumes Robotaxi commercialization without material regulatory headwinds—a scenario unsupported by
current NHTSA investigation status and FSD safety record deterioration (236 crashes in July CY25,
four fatalities). My bear target ($92) applies a distress multiple to core auto earnings excluding
credits. Bull case ($398) requires successful Robotaxi launch with approved commercial operations by
Q1-CY27; this is possible but not probable given regulatory and technical execution risks.

COMPUTED
The computed reads follow under their labels, up to SUPPORTED ACTIONS; each is derived from the
holding's data by fixed formulas.

COMPUTED ACTION
The rung a fixed rule gives from the computed reads: trim.

GRADE
A letter from A to F, derived from the computed quality, valuation and risk scores.
F.

PRICE BANDS (USD, with the move each implies from the current price; the analyst's expected price
from VERDICT beside each)
- three-month: bear 229.41 (-36.1%) / base 310.01 (-13.6%) / bull 390.61 (+8.8%); analyst none.
Method: base = the twelve-month base price return prorated to three months; bear and bull = ±26.0%
(two standard deviations of daily volatility over 63 sessions, capped at 26%).
- twelve-month: bear 62.04 (-82.7%) / base 163.12 (-54.6%) / bull 911.74 (+154.0%); analyst 145.00
(-59.6%). Method: consensus forward EPS (low / mid / high) × P/E multiples at the 75th / 50th / 25th
percentile of their spread to the 10-year Treasury over the last 12 quarterly observations. Notes:
the driver blends two consensus rows.
- three-year: bear 62.04 (-82.7%) / base 163.12 (-54.6%) / bull 911.74 (+154.0%); analyst none.
Method: the twelve-month drivers held at flat growth for two further years (a single forward
consensus row, or no definable growth) at the same multiples — an extrapolation that assumes today's
rate and spread regime holds.

CAPITAL EFFICIENCY
The computed twelve-month total return in each scenario (the move from the current price to the
scenario price, plus forward income per share, as a fraction of the current price) and the hurdle
rate it is measured against.
bear -82.6% / base -54.3% / bull +155.7%; hurdle 12.7%.

SUPPORTED ACTIONS
The rungs a fixed rule over the holding's reads supports, listed in full: sell-all, trim, hold. A
rung not listed is outside that rule.

INVESTOR PROFILE
- objective: maximize profit (total return; no income or capital-preservation mandate)
- risk tolerance: aggressive (medium-to-high)
- horizon: long-term (durable multi-quarter / multi-year theses)

======== PART 2: TASK ========

Determine the following from the inputs and return them as one JSON object in the shape at the end,
with no code fence and no surrounding text.

1. action — one rung for this holding, from these inputs alone: "sell-all", "trim", "hold", "add" or
"add-aggressively". The rung alone: no share count, dollar amount or portfolio weight. Decide it
from VERDICT and COMPUTED first, refined by POSITION, SUPPORTED ACTIONS and INVESTOR PROFILE. An
aggressive risk tolerance admits add-aggressively where the other inputs support it. Where even the
bull case under CAPITAL EFFICIENCY misses the hurdle and the forward read is poor, lean toward
realizing some or all of the position.

2. rationale — one sentence giving the single investment reason for the rung. Name the returns you
weighed by their values; do not describe them by their relation to another figure.

RETURN SHAPE (every value is a placeholder)
{"action":"<sell-all|trim|hold|add|add-aggressively>","rationale":""}
~~~~

## Response schema (`format`)

~~~~json
{
  "properties": {
    "action": {
      "enum": [
        "sell-all",
        "trim",
        "hold",
        "add",
        "add-aggressively"
      ],
      "type": "string"
    },
    "rationale": {
      "type": "string"
    }
  },
  "required": [
    "action",
    "rationale"
  ],
  "type": "object"
}
~~~~

## Variant — tax-exempt profile

The lines that differ from the user message above when the profile is not tax-sensitive.

~~~~text
(no line differs)
~~~~

## Variant — cost basis tripled

The lines that differ from the user message above when the position's cost basis is three times higher: the gain becomes a loss, under POSITION alone.

~~~~text
- Shares held: 10. Cost basis: $2,871.76. Market value: $3,589.70. Unrealized gain: $717.94 (+25.0% of the cost basis). Change since the last pull: new.
+ Shares held: 10. Cost basis: $8,615.28. Market value: $3,589.70. Unrealized loss: $5,025.58 (-58.3% of the cost basis). Change since the last pull: new.
~~~~
