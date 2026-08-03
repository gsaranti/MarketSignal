# FMP light-EOD adjustment basis — desk probe (2026-08-02)

*Evidence record for the open question raised by the target-function calibration slice
(`b4467fc`): does FMP `/stable/historical-price-eod/light` — the anchor window's
second-rung price source when Stooq is throttled — serve prices on the same
adjustment basis as Stooq's daily bars (split-adjusted, dividend-unadjusted)?
Closed at the desk with six HTTP calls; no portfolio run, no code change.*

## Verdict

**FMP `historical-price-eod/light` `price` is split-adjusted and dividend-unadjusted —
the same basis as Stooq's documented convention.**
The second-rung substitution into `daily_closes` never mixes adjustment bases,
so anchor windows, volatility, and the dispersion floor read identically off either rung.
The fallback is clean as built; nothing to change.

## Method

The probe cross-compares FMP's `light` variant against FMP's **own**
`non-split-adjusted` and `dividend-adjusted` variants over windows where each
adjustment is unmistakable, then anchors the result to known raw closes.
Stooq itself was unreachable to a non-JavaScript client at probe time (see the
incidental finding below), but its convention needs no re-verification — it is
documented and was live-verified on the M5 pre-flight
([data-sources.md §Stooq](../data-sources.md#stooq)).

Two windows:

- **Split basis — NVDA across its 10-for-1 split (ex 2024-06-10).**
  If `light` were not split-adjusted, pre-split rows would read ~$1,150–1,225.
- **Dividend basis — MO (Altria) in late August / early September 2022.**
  ~3.9 years of ~7–8%-yield payouts separate that window from probe date;
  a dividend-adjusted series would sit far below the raw closes.

## Evidence

NVDA, `light` `price` vs `non-split-adjusted` `adjClose`:

| Date | light | non-split-adjusted |
| --- | --- | --- |
| 2024-06-03 | 115.00 | 1150.00 |
| 2024-06-04 | 116.44 | 1164.40 |
| 2024-06-05 | 122.44 | 1224.40 |
| 2024-06-06 | 121.00 | 1210.00 |
| 2024-06-07 | 120.89 | 1208.90 |
| 2024-06-10 | 121.79 | 121.79 |
| 2024-06-14 | 131.88 | 131.88 |

Pre-split `light` rows are exactly the unadjusted closes ÷ 10 (2024-06-07's 1208.90
matches NVDA's known raw close $1,208.88 to the cent of FMP's rounding), and the
two variants converge on the ex-date — so `light` **is split-adjusted**.

MO, `light` `price` vs `dividend-adjusted` `adjClose`:

| Date | light | dividend-adjusted |
| --- | --- | --- |
| 2022-08-29 | 45.71 | 33.42 |
| 2022-08-31 | 45.12 | 32.99 |
| 2022-09-02 | 45.00 | 32.90 |
| 2022-09-09 | 45.57 | 33.32 |

`light` matches MO's raw closes of the day while the dividend-adjusted variant sits
~27% lower — the accumulated payout since — so `light` is **not dividend-adjusted**.

## Incidental finding — Stooq browser-verification interstitial

At probe time Stooq answered every non-JavaScript request — plain `curl` and a
retry with realistic browser-like headers alike — with an HTTP 200 HTML
**JavaScript proof-of-work browser-verification interstitial** (SHA-256 challenge
posted to `/__verify`), not the daily-hits notice observed 2026-07-31.
This is a second, distinct 200-HTML body in the CSV's place; whether it is a
permanent gate, an IP-scoped flag, or a temporary rollout is not determinable
from here.
The app's behavior is already correct against it: `parse_daily_csv` classifies
**any** HTML body in the CSV's place as the typed `StooqThrottled`
(`stooq.rs` — deliberately generic), so the first fetch trips the run-wide
breaker and the run rides the FMP second rung, which this probe verifies is
basis-identical.
Two consequences to watch, neither acted on now:
the `StooqThrottled` display string ("daily-hits limit reached") may misattribute
the cause when this interstitial is what actually answered;
and if the gate proves permanent, the FMP rung is de facto primary for deep
history — the data-health roll-up's deep-history line on the next live run is
the confirming signal.
No proof-of-work solver was or will be scripted around the gate.

## Probe hygiene

Six HTTP calls total: four FMP (`light` ×2, `dividend-adjusted` ×1,
`non-split-adjusted` ×1 — paid key, negligible against the plan cap) and two
Stooq CSV attempts (paced ≥ 2 s apart, abandoned on the interstitial).
