#!/usr/bin/env python3
"""M5 pre-flight: in-house RULER-style long-context probe.

Builds a synthetic financial dossier at several target token sizes, plants:
  - 3 needle facts (10% / 50% / 90% depth) — simple retrieval
  - 1 multi-hop chain (3 scattered links) — subsidiary -> CFO -> prior employer metric
  - 1 aggregation (3 scattered segment capex figures to sum)
then asks all questions in one call per size and scores by exact-value match.

Usage: longctx_probe.py [sizes-in-ktokens...]   default: 8 32 64 96 128 160
"""
import json, random, sys, urllib.request

BASE = "http://localhost:11434/api/chat"
MODEL = "qwen3.5:122b-a10b"
CHARS_PER_TOK = 3.6  # rough; actual reported via prompt_eval_count

SECTORS = ["industrial automation", "specialty chemicals", "regional banking",
           "medical devices", "enterprise software", "midstream energy",
           "consumer staples", "semiconductor equipment", "reinsurance", "logistics"]
VERBS = ["expanded", "compressed", "stabilized", "deteriorated", "re-rated", "normalized"]
NOUNS = ["gross margins", "order backlogs", "working-capital cycles", "credit spreads",
         "inventory turns", "free-cash conversion", "pricing power", "channel inventories"]

def filler_paragraph(rng, i):
    s = rng.choice(SECTORS); v = rng.choice(VERBS); n = rng.choice(NOUNS)
    pct = round(rng.uniform(0.4, 9.7), 1)
    q = rng.choice(["Q1", "Q2", "Q3", "Q4"])
    yr = rng.choice([2023, 2024, 2025])
    return (f"Note {i}: Across the {s} cohort, {n} {v} through {q} {yr}, moving roughly "
            f"{pct}% against the prior period. Management commentary emphasized capacity "
            f"discipline and mix effects, while sell-side revisions lagged the reported trend. "
            f"Channel checks suggested the move was broad-based rather than share-driven, and "
            f"the derivative read for adjacent suppliers remained ambiguous pending the next print. ")

# Planted facts (unique, non-inferable values)
NEEDLES = [
    ("The Oberhausen facility's Q3 maintenance capex came to $4.73 million.",
     "What was the Oberhausen facility's Q3 maintenance capex?", "4.73"),
    ("Kestrel Dynamics carried an unhedged yen exposure of 312 million JPY at quarter end.",
     "What was Kestrel Dynamics' unhedged yen exposure at quarter end?", "312"),
    ("The Tarragona plant reported a defect-adjusted yield of 87.6 percent in April.",
     "What defect-adjusted yield did the Tarragona plant report in April?", "87.6"),
]
HOP_LINKS = [
    "Meridian Foods' wholly owned subsidiary is Cobalt Harvest Ltd.",
    "Cobalt Harvest Ltd. is led by CFO Ingrid Vasterling.",
    "Before Cobalt Harvest, Ingrid Vasterling ran treasury at Pallas Freight, where she cut net leverage to 1.4x.",
]
HOP_Q = ("Trace the chain: Meridian Foods' subsidiary, that subsidiary's CFO, and the net "
         "leverage figure that CFO achieved at their previous employer. Give the final number.")
HOP_ANS = "1.4"
AGG_FACTS = [
    "Segment Alpha booked expansion capex of $21 million this year.",
    "Segment Beta booked expansion capex of $34 million this year.",
    "Segment Gamma booked expansion capex of $17 million this year.",
]
AGG_Q = "Sum the expansion capex across Segments Alpha, Beta, and Gamma. Give the total in millions."
AGG_ANS = "72"

def build_dossier(target_toks, rng):
    target_chars = int(target_toks * 1000 * CHARS_PER_TOK)
    paras, total, i = [], 0, 0
    while total < target_chars:
        p = filler_paragraph(rng, i)
        paras.append(p); total += len(p); i += 1
    n = len(paras)
    # depth positions for the 3 needles
    for needle, pos in zip((NEEDLES[0][0], NEEDLES[1][0], NEEDLES[2][0]),
                           (int(n * 0.10), int(n * 0.50), int(n * 0.90))):
        paras.insert(min(pos, len(paras) - 1), needle + " ")
    # scatter hop links at 20/55/85 and agg facts at 15/45/75
    for text, frac in [(HOP_LINKS[0], 0.20), (HOP_LINKS[1], 0.55), (HOP_LINKS[2], 0.85),
                       (AGG_FACTS[0], 0.15), (AGG_FACTS[1], 0.45), (AGG_FACTS[2], 0.75)]:
        paras.insert(min(int(len(paras) * frac), len(paras) - 1), text + " ")
    return "".join(paras)

QUESTIONS = [NEEDLES[0][1], NEEDLES[1][1], NEEDLES[2][1], HOP_Q, AGG_Q]
EXPECTED = [NEEDLES[0][2], NEEDLES[1][2], NEEDLES[2][2], HOP_ANS, AGG_ANS]

def call(payload, timeout=3600):
    req = urllib.request.Request(BASE, data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())

def run_size(ktoks, think=True):
    rng = random.Random(20260728 + ktoks)
    dossier = build_dossier(ktoks, rng)
    qs = "\n".join(f"Q{i+1}. {q}" for i, q in enumerate(QUESTIONS))
    prompt = (f"Below is an internal research dossier. Read it, then answer the questions "
              f"at the end. Answer each on its own line as 'A<n>: <answer>' with the bare "
              f"number where a number is asked.\n\n=== DOSSIER ===\n{dossier}\n=== END ===\n\n{qs}")
    num_ctx = min(int(ktoks * 1000 * 1.15) + 40960, 262144)  # prompt + thinking + output headroom, capped at native
    resp = call({"model": MODEL, "messages": [{"role": "user", "content": prompt}],
                 "think": think, "stream": False,
                 "options": {"temperature": 1.0, "top_p": 0.95, "top_k": 20, "min_p": 0.0,
                             "presence_penalty": 1.5, "num_ctx": num_ctx}})
    content = resp.get("message", {}).get("content", "")
    scores = []
    for i, exp in enumerate(EXPECTED):
        line = next((l for l in content.splitlines() if l.strip().lower().startswith(f"a{i+1}")), "")
        scores.append("PASS" if exp in line else f"FAIL (got: {line[:80]!r})")
    return {
        "target_ktoks": ktoks,
        "actual_prompt_tokens": resp.get("prompt_eval_count"),
        "num_ctx": num_ctx,
        "prompt_eval_ms": resp.get("prompt_eval_duration", 0) // 1_000_000,
        "gen_tokens": resp.get("eval_count"),
        "gen_ms": resp.get("eval_duration", 0) // 1_000_000,
        "needle_10pct": scores[0], "needle_50pct": scores[1], "needle_90pct": scores[2],
        "multi_hop": scores[3], "aggregation": scores[4],
    }

def main():
    sizes = [int(a) for a in sys.argv[1:]] or [8, 32, 64, 96, 128, 160]
    for k in sizes:
        try:
            print(json.dumps(run_size(k)), flush=True)
        except Exception as e:
            print(json.dumps({"target_ktoks": k, "error": str(e)[:300]}), flush=True)

if __name__ == "__main__":
    main()
