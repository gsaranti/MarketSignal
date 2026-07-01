# Current session handoff

## What happened

A **documentation session** (not build): researched local-model best-practices
and shipped **`docs/local-model-operations.md`** — the operational reference for
the primary local reasoner **`Qwen3.5-122B-A10B`** (context/effective-context,
thinking mode, per-mode sampling, `num_ctx`, structured-output gotchas, serving
pre-flight). Committed + pushed to `main` (`7044c4d`); reconciled
`local-models.md` / `.metis/BUILD.md` / `.metis/INDEX.md`. **4 Codex rounds**
resolved.

**Settled (don't re-litigate):** `Qwen3.5-122B-A10B` is **real** (Feb-2026 release,
*after* the Jan-2026 cutoff — that's why it read as unrecognized) and **locked as
the v2 local default** after a field survey; gpt-oss-120b was demoted *for us* (its
"harmony" format fights Ollama's `format` → unreliable schema JSON). The lighter
**Qwen3.6-35B-A3B** is the front challenger but **deliberately kept out of the
repo — re-evaluate after v2** (user's call).

**Two runtime findings (don't re-derive; both M5-gated):** (1) the 122B is **not
MLX-accelerated in Ollama yet** (only the 35B-A3B is) → it runs on the **llama.cpp
Metal/GGUF** fallback, where the multimodal `mmproj` load issue lives;
(2) **`format`×thinking is asymmetric** — `think:true`+`format` *composes*,
**`think:false`+`format` = bug #14645** (schema silently ignored) → any
schema-constrained distillation must **stay thinking-on** until #14645 is verified
fixed.

## Current state

On `main` @ `7044c4d`, pushed, in sync with origin. **Docs-only — no code
changed.** All prose + `.metis/` docs reconciled to the #14645 rule and the
MLX-where-supported serving wording. The new doc is **research-derived / M5-gated**
(every claim tagged vendor / community / verify-on-M5); its pre-flight checklist is
what runs when the M5 arrives. **Build order unchanged** — this was a doc detour,
not a build step. (Detail also in memory `local-model-operational-reference.md`.)

## Open questions

- **NEW — local-model runtime pre-flight (M5):** does the 122B load on our Ollama
  version, and on *which* backend (MLX vs Metal/GGUF — does the `mmproj` GGUF issue
  bite); is #14645 fixed (else keep `format` calls thinking-on); does `format`
  actually constrain; set `num_ctx` explicitly (≥48 GiB auto-default is ~256K);
  measure throughput. Full checklist in `docs/local-model-operations.md`.
- **M5-calibration (carried):** Stooq 8 PM-ET / 24h refresh, ~4wk
  `continuity_weight` bands + Research-stale threshold, tripwire thresholds, DTO
  deep-research budget default, leftover-budget oldest-N ordering, archive
  retention 100 + upside-exhausted threshold.
- **Four-part verdict model + bidirectional-conviction bound** (carried): lands
  when full Portfolio + TO are built.
- §1 **genuinely-open drafts** (carried): dead-money hurdle, feasible-set bounding;
  TO risk-tier / horizon / hypothesis-score / quota / gate tables.
- Standing **M5-gated backlog** (carried): web-research provisioning / gating / UI
  + rendered-retrieval, analytical-register live-check, no new Tavily, FMP-tier.

## Where to start

Unchanged from last session — begin the **live Schwab OAuth slice** (next in build
order; unaffected by this session's docs work; `schwab-integration.md` audited
clean: OAuth loopback, 30-min/7-day tokens, Keychain, positions + option chains).
**Check code-vs-doc first** on any Portfolio/TO formula — PR #45 already implements
the MVP engine math. The local-model runtime validation above is **M5-gated**, not
a next-session coding task.
