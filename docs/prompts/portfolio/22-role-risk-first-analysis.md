# Role/risk interpretation — first analysis

*Generated from the code at `portfolio-v49` by `fixed_evidence::prompt_examples`; regenerate rather than edit (`docs/prompts/README.md`).*

Holding: BND, the synthetic total bond market ETF the fixed evidence set carries for the role/risk branch.
The role/risk branch of the intrinsic verdict, taken for a vehicle class the engine cannot price: the fund readout stands where the computed scores would, and the model returns the role read and the ledger, never a grade or a target.
The hand-written Treasury positioning line and the venue put/call backdrop render because the synthetic dossier carries them.
Every `[stub: …]` marks a parsed article or research field whose prose the run fills; the ids, dates, URLs and headers around it are as rendered.

## Request

| Field | Value |
| --- | --- |
| Workflow step | `docs/portfolio-workflow.md` §Step 6f |
| Stage label | `role-risk BND` |
| Model | the roster's resident reasoner |
| Thinking | on (`think: true`) |
| Options | `{"min_p":0.0,"num_ctx":131072,"num_predict":65536,"presence_penalty":1.5,"temperature":1.0,"top_k":20,"top_p":0.95}` |
| Output protocol | the JSON schema below as the `format` grammar; no tools |
| Residency | `keep_alive: -1` (stays resident) |
| Prompt material | 6275 chars — the messages and tools as serialized |

## System message

~~~~text
You are an investment analyst producing an independent read of one fund holding for a portfolio review. Part 1 of the message gives the inputs. Part 2 states what to determine from them and the shape to return. You will return role_summary and ledger, as one JSON object.
~~~~

## User message (5718 chars)

~~~~text
======== PART 1: INPUTS ========
HOLDING
BND (VANGUARD TOTAL BOND MARKET ETF).
Price: $72.38 per share.
Date: 2026-09-16.
This is the first analysis of this holding.

CLASS
bond fund. Reported asset class: Fixed Income.

EXPOSURE TILT
The fund's largest weights, by sector where reported and otherwise by country.
- United States: 94.0%
- Supranational: 2.0%
- Canada: 1.0%

UNDERLYING POSITIONING (CFTC weekly, as of 2026-09-09)
10-Year US Treasury Note — speculator net -650000 contracts (18.4% of OI long), w/w -22000; asset-manager net +1200000 (w/w +15000)

RISK PROFILE
Annualized realized volatility: 0.022 (a fraction; 0.14 means 14% a year).
Market-wide options sentiment (CBOE daily put/call, as of 2026-09-16): total 0.95, index 1.21, equity 0.62

EVIDENCE GAPS
no duration, credit or yield-curve data for this fund

FINANCIAL METRICS
The market metrics are daily; the expense ratio is the fund's published figure.
Each metric has a label in brackets and a confirmation rule, the number of prints past a level that count as a crossing; both are used by the ledger in Part 2.
- daily realized return volatility [return-volatility]: 0.0014 — a daily fraction, never a percent (0.02 means 2% per day); confirmed by two consecutive daily closes
- trailing price return [trailing-return]: -0.0142 — a fraction, never a percent (0.16 means 16%); confirmed by two consecutive daily closes
- fund expense ratio [expense-ratio]: 0.0003 (0.03%/yr) — a fraction of assets per year, never a percent (0.0075 means 0.75%); confirmed by one filing
- the holding's price (account currency) [price]: 72.38 — dollars per share; confirmed by two consecutive daily closes

RESEARCH SUMMARY
[stub: the distilled research — the fund's exposure profile and holdings news, as the reduce call returned them]

MARKET ANALYSIS
A market-level analysis dated 2026-09-16, followed by the stance of the 3 most recent analyses.
## Market Signal Thesis

[stub: the latest report's Market Signal Thesis section]

## Investment Strategy

[stub: the latest report's Investment Strategy section]

- 2026-09-16: thesis bearish, risk posture risk-off
- 2026-08-30: thesis mixed, risk posture mixed
- 2026-08-25: thesis mixed, risk posture mixed

PRIOR THESIS LEDGER
None: this is the first analysis.

======== PART 2: TASK ========

Determine the following from the inputs and return them as one JSON object in the shape at the end, with no code fence and no surrounding text.

1. role_summary — a few sentences on the vehicle's mandate, the exposure it exists to supply, and the cost and risk of holding it, from CLASS, EXPOSURE TILT, RISK PROFILE, EVIDENCE GAPS, FINANCIAL METRICS and RESEARCH SUMMARY.

2. ledger — the position's initial thesis ledger:
   - thesis: the standing thesis, in a few sentences, drawing on MARKET ANALYSIS for the market setup.
   - key_drivers: what the thesis depends on — for a fund, the exposure it supplies, its cost and its fidelity to its mandate. Where a driver is one of the labelled metrics in FINANCIAL METRICS, series is its label; otherwise series is null.
   - base, bear, bull: the conditions that define each case, with a probability in percent; the three sum to about 100.
   - what_must_improve: what has to improve for the bull case. what_must_not_break: what has to hold for the base case.
   - falsifiers: the observations that would show the thesis wrong. technology_class is true only for a third party's technology event (a competitor's or supplier's product or standard) and false otherwise. tripped is false.
   - triggers: pre-committed conditions for trimming or selling, with family "trim" or "sell". fired is false.

   Every falsifier and trigger has a quant field.
   A condition on one labelled metric is quantitative: quant holds series (one of return-volatility, trailing-return, expense-ratio, price), comparator ("below" or "above"), threshold and margin, and statement is a short name for the condition, without a figure. It is a single level on a single metric, one the metric has not already crossed, with no duration, volume or second condition ("for two weeks", "on elevated volume", "unless …").
   Any other condition is qualitative: quant is null, and statement is the observation itself, specific enough to be researched. A condition that needs a duration, volume or second condition is qualitative.
   threshold: the level, in the metric's unit ("above 0.75%" on expense-ratio is 0.0075).
   margin: the noise around the threshold that a crossing must clear, in the same unit — small relative to the level, for example 2 on a price of 100, 0.002 on a daily volatility of 0.02, or 0.0005 on an expense ratio of 0.0075.
   Example, quantitative: statement "Price support", quant {"series": "price", "comparator": "below", "threshold": 38, "margin": 0.4}.
   Example, qualitative: statement "The mandate drifts from the stated index methodology", quant null.

RETURN SHAPE (every value is a placeholder; an array holds as many items as apply)
{"role_summary":"","ledger":{"thesis":"","key_drivers":[{"name":"","series":"<return-volatility|trailing-return|expense-ratio|price|null>"}],"base":{"conditions":"","probability_pct":0},"bear":{"conditions":"","probability_pct":0},"bull":{"conditions":"","probability_pct":0},"what_must_improve":"","what_must_not_break":"","falsifiers":[{"statement":"","quant":{"series":"<return-volatility|trailing-return|expense-ratio|price>","comparator":"<below|above>","threshold":0,"margin":0},"technology_class":false,"tripped":false}],"triggers":[{"statement":"","family":"<trim|sell>","quant":{"series":"<return-volatility|trailing-return|expense-ratio|price>","comparator":"<below|above>","threshold":0,"margin":0},"fired":false}]}}
~~~~

## Response schema (`format`)

~~~~json
{
  "properties": {
    "ledger": {
      "properties": {
        "base": {
          "properties": {
            "conditions": {
              "type": "string"
            },
            "probability_pct": {
              "type": "number"
            }
          },
          "required": [
            "conditions",
            "probability_pct"
          ],
          "type": "object"
        },
        "bear": {
          "properties": {
            "conditions": {
              "type": "string"
            },
            "probability_pct": {
              "type": "number"
            }
          },
          "required": [
            "conditions",
            "probability_pct"
          ],
          "type": "object"
        },
        "bull": {
          "properties": {
            "conditions": {
              "type": "string"
            },
            "probability_pct": {
              "type": "number"
            }
          },
          "required": [
            "conditions",
            "probability_pct"
          ],
          "type": "object"
        },
        "falsifiers": {
          "items": {
            "properties": {
              "quant": {
                "properties": {
                  "comparator": {
                    "enum": [
                      "below",
                      "above"
                    ],
                    "type": "string"
                  },
                  "margin": {
                    "type": "number"
                  },
                  "series": {
                    "enum": [
                      "return-volatility",
                      "trailing-return",
                      "expense-ratio",
                      "price"
                    ],
                    "type": "string"
                  },
                  "threshold": {
                    "type": "number"
                  }
                },
                "required": [
                  "series",
                  "comparator",
                  "threshold",
                  "margin"
                ],
                "type": [
                  "object",
                  "null"
                ]
              },
              "statement": {
                "type": "string"
              },
              "technology_class": {
                "type": "boolean"
              },
              "tripped": {
                "type": "boolean"
              }
            },
            "required": [
              "statement",
              "quant",
              "technology_class",
              "tripped"
            ],
            "type": "object"
          },
          "type": "array"
        },
        "key_drivers": {
          "items": {
            "properties": {
              "name": {
                "type": "string"
              },
              "series": {
                "enum": [
                  "return-volatility",
                  "trailing-return",
                  "expense-ratio",
                  "price",
                  null
                ],
                "type": [
                  "string",
                  "null"
                ]
              }
            },
            "required": [
              "name",
              "series"
            ],
            "type": "object"
          },
          "type": "array"
        },
        "thesis": {
          "type": "string"
        },
        "triggers": {
          "items": {
            "properties": {
              "family": {
                "enum": [
                  "trim",
                  "sell"
                ],
                "type": "string"
              },
              "fired": {
                "type": "boolean"
              },
              "quant": {
                "properties": {
                  "comparator": {
                    "enum": [
                      "below",
                      "above"
                    ],
                    "type": "string"
                  },
                  "margin": {
                    "type": "number"
                  },
                  "series": {
                    "enum": [
                      "return-volatility",
                      "trailing-return",
                      "expense-ratio",
                      "price"
                    ],
                    "type": "string"
                  },
                  "threshold": {
                    "type": "number"
                  }
                },
                "required": [
                  "series",
                  "comparator",
                  "threshold",
                  "margin"
                ],
                "type": [
                  "object",
                  "null"
                ]
              },
              "statement": {
                "type": "string"
              }
            },
            "required": [
              "statement",
              "family",
              "quant",
              "fired"
            ],
            "type": "object"
          },
          "type": "array"
        },
        "what_must_improve": {
          "type": "string"
        },
        "what_must_not_break": {
          "type": "string"
        }
      },
      "required": [
        "thesis",
        "key_drivers",
        "bear",
        "base",
        "bull",
        "what_must_improve",
        "what_must_not_break",
        "falsifiers",
        "triggers"
      ],
      "type": "object"
    },
    "role_summary": {
      "type": "string"
    }
  },
  "required": [
    "role_summary",
    "ledger"
  ],
  "type": "object"
}
~~~~
