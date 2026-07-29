#!/usr/bin/env python3
"""M5 pre-flight: schema-integrity (#14645) + thinking checks against Ollama /api/chat.

Tests (per docs/local-model-operations.md):
  A. think:false + format  x N  — the historically-bugged config (probabilistic ~1/3 failure pre-fix)
  B. think:true  + format       — the safe composing path (thinking field + schema-valid content)
  C. think:true, no format      — reasoning trace appears, free text answer
  D. two-step reason -> distill (format + think:true, temp 0.6 precise row) -> schema-valid
  E. malformed format value     — must be REJECTED (HTTP error), not silently passed through
  F. think:true + tools + format x N — uncorroborated v0.20.2 report: format ignored when tools present
"""
import json, sys, urllib.request, urllib.error

BASE = "http://localhost:11434/api/chat"
MODEL = "qwen3.5:122b-a10b"
NUM_CTX = 16384

SCHEMA = {
    "type": "object",
    "properties": {
        "reasoning": {"type": "string"},
        "ticker": {"type": "string"},
        "verdict": {"type": "string", "enum": ["buy", "hold", "sell"]},
        "confidence": {"type": "number", "minimum": 0, "maximum": 1},
    },
    "required": ["reasoning", "ticker", "verdict", "confidence"],
    "additionalProperties": False,
}

PROMPT = (
    "Acme Corp (ticker ACME) grew revenue 12% YoY with stable 18% operating margins, "
    "but trades at 42x forward earnings versus a sector median of 21x. "
    "Give your read as a JSON object with fields: reasoning, ticker, verdict (buy/hold/sell), confidence (0-1)."
)

OPTS_THINK_GENERAL = {"temperature": 1.0, "top_p": 0.95, "top_k": 20, "min_p": 0.0,
                      "presence_penalty": 1.5, "num_ctx": NUM_CTX}
OPTS_THINK_PRECISE = {"temperature": 0.6, "top_p": 0.95, "top_k": 20, "min_p": 0.0,
                      "presence_penalty": 0.0, "num_ctx": NUM_CTX}
OPTS_NONTHINK_GENERAL = {"temperature": 0.7, "top_p": 0.8, "top_k": 20, "min_p": 0.0,
                         "presence_penalty": 1.5, "num_ctx": NUM_CTX}

TOOLS = [{
    "type": "function",
    "function": {
        "name": "fetch_quote",
        "description": "Fetch the latest quote for a ticker",
        "parameters": {
            "type": "object",
            "properties": {"symbol": {"type": "string"}},
            "required": ["symbol"],
        },
    },
}]


def call(payload, timeout=600):
    req = urllib.request.Request(BASE, data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, {"error": e.read().decode(errors="replace")}


def validate(obj):
    """Hand-rolled check against SCHEMA (no deps)."""
    errs = []
    if not isinstance(obj, dict):
        return ["not an object"]
    for k in SCHEMA["required"]:
        if k not in obj:
            errs.append(f"missing {k}")
    for k in obj:
        if k not in SCHEMA["properties"]:
            errs.append(f"extra key {k}")
    if not isinstance(obj.get("reasoning"), str):
        errs.append("reasoning not str")
    if not isinstance(obj.get("ticker"), str):
        errs.append("ticker not str")
    if obj.get("verdict") not in ("buy", "hold", "sell"):
        errs.append(f"verdict invalid: {obj.get('verdict')!r}")
    c = obj.get("confidence")
    if not isinstance(c, (int, float)) or isinstance(c, bool) or not (0 <= c <= 1):
        errs.append(f"confidence invalid: {c!r}")
    return errs


def content_of(resp):
    return resp.get("message", {}).get("content", "")


def thinking_of(resp):
    return resp.get("message", {}).get("thinking", "") or ""


def check_schema_valid(resp, label):
    raw = content_of(resp)
    try:
        obj = json.loads(raw)
    except Exception as e:
        return False, f"{label}: NOT JSON ({e}); first 200 chars: {raw[:200]!r}"
    errs = validate(obj)
    if errs:
        return False, f"{label}: JSON but schema-invalid: {errs}; obj keys: {list(obj)[:8]}"
    return True, f"{label}: schema-valid"


def main():
    n_reps = int(sys.argv[1]) if len(sys.argv) > 1 else 8
    results = {}

    # A: think:false + format (the bugged config pre-v0.32.0)
    fails = []
    for i in range(n_reps):
        status, resp = call({"model": MODEL, "messages": [{"role": "user", "content": PROMPT}],
                             "format": SCHEMA, "think": False, "stream": False,
                             "options": OPTS_NONTHINK_GENERAL})
        if status != 200:
            fails.append(f"rep{i}: HTTP {status}: {str(resp)[:200]}")
            continue
        ok, msg = check_schema_valid(resp, f"rep{i}")
        if not ok:
            fails.append(msg)
    results["A think:false+format"] = f"{n_reps - len(fails)}/{n_reps} clean" + (
        "; FAILURES: " + " | ".join(fails) if fails else "")

    # B: think:true + format
    status, resp = call({"model": MODEL, "messages": [{"role": "user", "content": PROMPT}],
                        "format": SCHEMA, "think": True, "stream": False,
                        "options": OPTS_THINK_PRECISE})
    if status != 200:
        results["B think:true+format"] = f"HTTP {status}: {str(resp)[:200]}"
    else:
        ok, msg = check_schema_valid(resp, "B")
        has_think = len(thinking_of(resp)) > 0
        results["B think:true+format"] = f"{msg}; thinking field {'POPULATED (' + str(len(thinking_of(resp))) + ' chars)' if has_think else 'EMPTY'}"

    # C: think:true, no format
    status, resp = call({"model": MODEL, "messages": [{"role": "user", "content": PROMPT}],
                        "think": True, "stream": False, "options": OPTS_THINK_GENERAL})
    if status != 200:
        results["C think:true no-format"] = f"HTTP {status}: {str(resp)[:200]}"
    else:
        tlen, clen = len(thinking_of(resp)), len(content_of(resp))
        results["C think:true no-format"] = (
            f"thinking {'POPULATED (' + str(tlen) + ' chars)' if tlen else 'EMPTY — FAIL'}; content {clen} chars")

    # D: two-step reason -> distill
    status, r1 = call({"model": MODEL, "messages": [{"role": "user", "content": PROMPT + " Reason it through in prose first; no JSON yet."}],
                      "think": True, "stream": False, "options": OPTS_THINK_GENERAL})
    if status != 200:
        results["D two-step"] = f"step1 HTTP {status}"
    else:
        prose = content_of(r1)
        status, r2 = call({"model": MODEL,
                           "messages": [{"role": "user", "content": "Distill this analysis into the JSON object (fields: reasoning, ticker, verdict buy/hold/sell, confidence 0-1):\n\n" + prose}],
                           "format": SCHEMA, "think": True, "stream": False,
                           "options": OPTS_THINK_PRECISE})
        if status != 200:
            results["D two-step"] = f"step2 HTTP {status}"
        else:
            ok, msg = check_schema_valid(r2, "D distill")
            results["D two-step"] = msg + f" (step1 prose {len(prose)} chars)"

    # E: malformed format value — expect rejection
    status, resp = call({"model": MODEL, "messages": [{"role": "user", "content": PROMPT}],
                        "format": {"type": "object", "properties": "THIS-IS-GARBAGE"},
                        "think": True, "stream": False, "options": OPTS_THINK_PRECISE})
    if status != 200:
        results["E malformed schema"] = f"REJECTED with HTTP {status} (correct): {str(resp)[:150]}"
    else:
        results["E malformed schema"] = (
            f"HTTP 200 — SILENTLY ACCEPTED (bad); content first 150: {content_of(resp)[:150]!r}")

    # F: think:true + tools + format
    fails = []
    for i in range(n_reps):
        status, resp = call({"model": MODEL, "messages": [{"role": "user", "content": PROMPT + " Do not call tools; answer directly."}],
                             "format": SCHEMA, "think": True, "tools": TOOLS, "stream": False,
                             "options": OPTS_THINK_PRECISE})
        if status != 200:
            fails.append(f"rep{i}: HTTP {status}: {str(resp)[:150]}")
            continue
        if resp.get("message", {}).get("tool_calls"):
            fails.append(f"rep{i}: model called a tool despite instruction (not a format failure)")
            continue
        ok, msg = check_schema_valid(resp, f"rep{i}")
        if not ok:
            fails.append(msg)
    results["F think+tools+format"] = f"{n_reps - len(fails)}/{n_reps} clean" + (
        "; FAILURES: " + " | ".join(fails) if fails else "")

    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
