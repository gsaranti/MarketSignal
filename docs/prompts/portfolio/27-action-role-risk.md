# Action — role/risk branch

*Generated from the code by `fixed_evidence::prompt_examples`; last changed at `portfolio-v71`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: BND, the synthetic total bond market ETF the fixed evidence set carries for the role/risk branch.
The action call is the investor profile's one entry point: the finished verdict, the holding's own evidence, the engine's supported set and the profile decide the rung and one rationale — never a comparison with other holdings.
On the role/risk branch the readout's computed sections stay top-level under no COMPUTED heading and the thesis document stands in for the graded verdict — VERDICT carries the document alone — and the engine set is the reduced ladder (sell-all, trim, hold).
POSITION renders the synthetic fund's ten units the same way as a stock's.
The document is the offline stub's on the synthetic fund, assembled into the verdict by the pipeline's own seam.
This packet renders no article or research text, so it carries no stub.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6f |
| Stage label | `action BND` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | the JSON schema below as the `format` grammar; no tools |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 3175 chars — the messages and tools as serialized |

## System message

~~~~text
You are an equity analyst deciding the portfolio action for one holding in a portfolio review. You
will return action and rationale, as one JSON object. Part 1 of the message gives the inputs. Part 2
defines those outputs and gives the shape to return.
~~~~

## User message (2780 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
BND (VANGUARD TOTAL BOND MARKET ETF).
Price: $72.38 per share.
Date: 2026-09-16.

POSITION
The holding as the account carries it: the shares held, the total cost basis, the market value, the
unrealized gain or loss (the market value less the cost basis, and as a share of a positive cost
basis; not available where no basis is reported) and the change in the shares held since the last
pull — new where the last pull had none, else increased, decreased or unchanged, with the shares
held then and now.
Shares held: 10. Cost basis: $730.00. Market value: $723.80. Unrealized loss: $6.20 (-0.8% of the
cost basis). Change since the last pull: new.

CLASS
bond fund

EXPOSURE TILT
United States 94%, Supranational 2%, Canada 1%

RISK PROFILE
Expense drag: 0.0003 (0.03%/yr) of assets per year. Observable risk: 0.022 (annualized realized
volatility). Structural flag (leveraged / inverse or option-overlay path dependency): no.

EVIDENCE GAPS
no duration, credit or yield-curve data for this fund

VERDICT
An analyst's read of the holding's data and research: the thesis document.
Thesis document:
Role: bond fund supplying United States exposure; held for its portfolio role.

Risks: the expense drag, the structural path dependency where one applies, and the exposure drifting
from its mandate.

Triggers: trim on an expense ratio above 0.75% at the next published figure; sell on a mandate
change.

Summary: the vehicle supplies the exposure it exists to supply at its reported cost.

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
from CLASS, VERDICT, EXPOSURE TILT and RISK PROFILE first, refined by EVIDENCE GAPS, POSITION,
SUPPORTED ACTIONS and INVESTOR PROFILE. An aggressive risk tolerance admits add-aggressively where
the other inputs support it. An add-side rung needs support from the vehicle's own attributes,
stated in the rationale.

2. rationale — one sentence giving the single investment reason for the rung.

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
