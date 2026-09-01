# Local Model Operational Reference — Qwen3.5-122B-A10B

Operational best-practices for the local suite's primary reasoner.
This is the **how to run it well** companion to [local-models.md](local-models.md) (which covers the *architecture* — the serving runtime, roster, adapter seam, and context-memory discipline).
Everything here concerns the one model the roster defaults to for every reasoning role: **`Qwen3.5-122B-A10B`**.

**Status — research-derived, not yet live-validated.**
Every figure below is from vendor documentation (the Hugging Face model card, Qwen docs) or community runtime sources, gathered while the project is hardware-gated on the M5 (the M1 dev machine can't host the model).
Claims are tagged **[vendor]** (documented by Qwen / a model card), **[community]** (runtime reports, treat as directional), or **[verify on M5]** (a live pre-flight check, not yet confirmed).
Re-validate the **[verify on M5]** items before the first real local run.
A **2026-07-07 adversarially-verified web survey** (primary sources: Ollama release notes, library tags, GitHub issue/PR threads) refreshed the serving-stack facts below; those carry a dated **[verified 2026-07-07]** tag.

## Why this model (compact)

`Qwen3.5-122B-A10B` is the default because it is the open-weight model that best satisfies the suite's hard constraints simultaneously: **open / keyless** (Apache 2.0), **fits a 128 GB Apple-Silicon machine** with the embedder co-resident, a **262 K context window**, a real **thinking mode**, and — the load-bearing property for a schema-validated pipeline — **reliable grammar-constrained JSON** on the Qwen family (the alternatives that fit either gave up structured-output reliability on Ollama or didn't fit cleanly).
This choice is worth revisiting after v2 ships; it is not permanent.

**Re-validated 2026-07-07:** a fresh adversarially-verified field survey surfaced no challenger in the ~80–130B class.
GLM-Air-class models stay disqualified on *Ollama serving fidelity*, not model quality — GLM-4.5-Air is a community-port-only model whose template substitutes a Qwen-style JSON tool envelope for the XML envelope it was trained on (the port's own page warns behavior may vary), Ollama's Go template engine can't faithfully reproduce its Jinja chat template (ollama#10222 open), and Ollama's native XML tool-call parsing still breaks in the wild (ollama#13820 open).
The official-library GLM-5/5.1 models ship first-party templates and are the named **re-benchmark candidate** once their structured-output reliability is assessed.
**[verified 2026-07-07]**

## Model at a glance [vendor]

- **Released** ~Feb 2026, **Apache 2.0**.
- **Sparse MoE:** 122 B total parameters, **10 B activated** per token; **256 experts** (8 routed + 1 shared); 48 layers, hidden dim 3072.
- **Hybrid attention:** Gated DeltaNet (linear) + Gated Attention (full) layers in roughly a 3:1 ratio — this is *why* it carries long context cheaply, and also why runtime support is newer/less universal than a plain-transformer model (see [§Serving](#serving--memory-apple-silicon-128-gb)).
- **Natively multimodal** (the 3.5/3.6 generation ships a vision encoder).
  **We use it text-only.**
  This matters operationally only because the multimodal packaging (a separate `mmproj` vision projector in GGUF builds) is the source of the Ollama loading caveat below.

## Context window

- **262,144 tokens native**, extensible to ~**1.01 M** via YaRN (RoPE scaling).
  **[vendor]**
- **We do not need YaRN.**
  The suite's context-memory discipline assembles compact, bounded per-item packets ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline)); 262 K native is far more than any packet uses, so the model runs at native scale and YaRN extension stays off (it costs accuracy at shorter lengths).
- **Effective context is well below the advertised window.**
  There is no published RULER curve for *this* model, so treat this as a **planning heuristic, not a measured property**: across long-context models generally, effective context often lands around **~50–65 % of the stated window** — they degrade on multi-hop reasoning and aggregation (our synthesis workload) long before the hard limit, even while acing simple needle-in-a-haystack retrieval.
  As a **conservative budget**, plan reliable use up to ~130–170 K and treat beyond as degrading rather than failing.
  **[community / researched judgment — not a vendor number]** A 2026-07-07 survey found **no published RULER/LongBench-style measurement** for this model, so the M5 pre-flight ran the in-house probe instead.
  **The probe result (2026-07-28, Q4_K_M, llama.cpp Metal): no degradation observed through 160.6 K prompt tokens** — needle retrieval at 10/50/90 % depth, a 3-link multi-hop chain, and a scattered 3-term aggregation all passed at every size tested (6 K / 23 K / 45 K / 68 K / 90 K / 113 K / 160.6 K; 35/35 checks).
  That measures retrieval, chaining, and aggregation — not the full synthesis workload — so keep the ~130–170 K conservative budget as the planning ceiling, now **supported by measurement to its midpoint** rather than resting on the generic 50–65 % heuristic.
  Full evidence record: [verification/2026-07-28-m5-preflight.md](verification/2026-07-28-m5-preflight.md).
  **[verified live 2026-07-28]**
- **Do not starve the window either.**
  The model card advises keeping context **≥ 128 K to preserve thinking capability** — thinking chains are long (tens of thousands of tokens), so a too-small window truncates reasoning.
  The practical target is a *generous* context that comfortably holds packet + thinking
  + output, not the smallest that fits.
    **[vendor]**

## Thinking mode

- **On by default**, emitting a `<think>…</think>` block before the answer.
  **[vendor]**
- **Disable per call** with `chat_template_kwargs: {"enable_thinking": false}` (on Ollama, the `think` parameter / `think: false`).
  **[vendor]**
- **No `/think` · `/nothink` soft switch.**
  This is a deliberate change from Qwen3 — 3.5 supports only the hard `enable_thinking` flag, so mode is chosen by the call, never by an inline token in the prompt.
  Our adapter already selects mode per stage, so this is a non-issue *as long as no stage relies on the soft switch.*
  **[vendor]**
- **Strip thinking from history.**
  In a multi-turn loop, exclude prior `<think>` blocks from the messages sent on later turns — the card is explicit that thinking content must not accumulate in conversation history.
  **[vendor]**
- **Generation length:** 32,768 tokens for most queries; up to 81,920 for the hardest multi-step problems.
  Size `num_ctx` to hold input + thinking + this.
  **[vendor]**

## Structured output × thinking — the one incompatibility that bites us

This is the single most load-bearing generation-side fact for our pipeline, because the suite requests every structured stage-to-stage contract through Ollama's grammar-constrained `format` and then app-validates the returned body ([local-models.md §Schema-constrained output](local-models.md#schema-constrained-output)), while the model defaults to thinking-on.
The mechanic is asymmetric: Ollama applies the `format` GBNF grammar mask **only after the end-of-thinking token**, so the two flags behave very differently together.

- **`think: true` + `format` *composes* — this is the safe path.**
  The model produces its reasoning, closes the thinking block, and the grammar then constrains the final answer; the app still parses and validates `content` before accepting it.
  Reasoning lands in the separate `message.thinking` field and the constrained object in `content` — you get both in one call when the grammar engages.
  **[community]**
- **`think: false` + `format` is BROKEN in every release through v0.31.2** — bug #14645, *"format is ignored when think is disabled for qwen3.5 series."*
  Root cause (maintainer-confirmed): Ollama defers the `format` grammar mask until a thinking→content transition that never fires when `think: false` (the qwen3-family template pre-closes the `<think>` block), so `format` never engages and the model returns **free-form text where you asked for schema-valid JSON** — the exact "parse-and-pray" failure the suite forbids, and *silent*.
  Worse, the failure is **probabilistic** (a 0.30.7 repro failed ~1 in 3 calls on `/api/chat`): a run of clean responses is model compliance, not enforcement.
  The trap: the intuitive "fast, non-thinking distill" call (`think: false` + `format`) is precisely the bugged configuration — and it is also how [local-models.md](local-models.md)'s "non-thinking distillation" mode would naively be wired.
  **The fix merged upstream 2026-07-07** (PR #15901, *"apply format constraint for all thinking parsers when think=false"*, commit `892e7f6`; issue closed by the maintainer) and **first ships in v0.32.0** (tagged 2026-07-11; the release notes don't name it, but the merge commit is confirmed an ancestor of the tag via the GitHub compare API).
  **[verified live on M5 2026-07-28, v0.32.5: 24/24 `think:false` + `format` calls returned schema-valid output (vs the historical ~1-in-3 failure rate — pass probability ≈ 0.006% were the bug live), and a malformed schema is rejected HTTP 400, not silently passed through — the fix *behaves*, not just ships]**
- **The fix is confirmed on the pinned v0.32.5, so non-thinking distillation is unlocked *on this version*.**
  The rule survives as a version discipline rather than a standing prohibition: every call that carries `format` may now run `think: false`, **but any Ollama version bump re-locks it until the schema-integrity check passes on the new version** — a run of clean responses on an unverified version is model compliance, not enforcement.
  **Never ship a `think: false` + `format` call on an unverified version.**
  On the verified pinned version, a `format`-carrying call runs `think: false` directly — the wired default mapping (see [§Sampling settings](#sampling-settings-vendor)).
  Two **fallback patterns** cover a re-locked (unverified-bump) version:
  1. **Two-step (heavy stages).**
     A thinking call reasons freely (no `format`), then a **second `format`-carrying call — thinking kept on** — distills into the schema object.
     This is the suite's research/interpretation → schema-distillation split with the distill call temporarily riding thinking-on until the new version re-verifies.
  2. **Reasoning-field-first (light stages).**
     For a stage wanting a little reasoning *and* structure in one call, put a `reasoning` string field **first** in the schema (`{"reasoning": "...", ...}`) so the model reasons into that field before the structured fields — naturally a thinking-on call.

  One additional repro was flagged before trusting `format` on the agentic path: a single uncorroborated report (v0.20.2) of `format` being ignored even with `think: true` **when `tools` are passed in the same call** — the shape the research loop *used to* issue.
  **[verified clean on M5 2026-07-28, v0.32.5: 8/8 `think:true` + `tools` + `format` calls schema-valid — the report does not reproduce at that sample]**
  At 47-holding scale, though, that combined shape did misbehave — the terminal turn returned empty or fenced bodies at ~70% (attempt-4 Finding 4) — so **fix B retired it**: the research gathering turns now carry `tools` with no `format`, and a separate synthesis call carries `format` with no `tools`, so the research path no longer relies on the two co-existing (`docs/verification/2026-08-31-big-run-attempt-4-findings.md` §Finding 4).
  The contrast between the clean short probe and the failed production shape means this is protocol isolation around the observed joint condition, not proof of a universal tools-plus-`format` incompatibility; the synthesis parser independently rejects missing grammar-required keys and blank findings/claim fields under the bounded schema retry.
  The growing gathering conversation is independently bounded: one response contributes at most 8 accepted tool calls, and the complete serialized messages plus tool schema are checked against the shared input budget before every issued call and retained result; a bound ends gathering as a recorded degradation and still takes the fresh synthesis call.

## Sampling settings [vendor]

Set these per call via the adapter `options`, switched by mode.
Greedy decoding is **explicitly warned against** — temperature 0 / disabled sampling drives the model into repetition loops and quality collapse.

| Mode | temperature | top_p | top_k | min_p | presence_penalty |
|---|---|---|---|---|---|
| Thinking — general | 1.0 | 0.95 | 20 | 0.0 | 1.5 |
| Thinking — precise/coding | 0.6 | 0.95 | 20 | 0.0 | 0.0 |
| Non-thinking — general | 0.7 | 0.8 | 20 | 0.0 | 1.5 |
| Non-thinking — reasoning | 1.0 | 1.0 | 40 | 0.0 | 2.0 |

- `presence_penalty` may be tuned 0–2 to curb repetition; **too high causes language-mixing and quality loss.**
- Default mapping for our stages: **research / interpretation → thinking-general**; **consolidation / distillation → non-thinking-general** (wired 2026-08-01 — the #14645 fix is verified on the pinned v0.32.5, so distillation sends an explicit `think: false`; the version-discipline rule above still re-locks a `format`-carrying `think: false` call on any unverified Ollama bump).
  The two rows ship as adapter option profiles (`local_model::options`), so stages never hand-roll sampling literals.

## Serving & memory (Apple Silicon, 128 GB)

The suite serves through **Ollama** ([local-models.md §Serving runtime](local-models.md#serving-runtime)).
Ollama added a genuine **MLX backend** on Apple Silicon in **v0.19** (Mar 2026), since made the **default on macOS arm64** (no longer an opt-in preview).
The caveat that lands directly on our choice: **MLX acceleration rolls out per-model, and the 122B-A10B is still not covered as of v0.32.5 (2026-07-28)** — release notes across the Jun 4 – Jul 28 window name Command A/North (v0.30.10) and Gemma 4 (v0.31.1) but no Qwen3.5, and the ollama.com library carries `-mlx` tags only for the 0.8b–35b sizes; every 122b tag is GGUF-only (`122b` / `122b-a10b` / `122b-a10b-q4_K_M`).
Default-on MLX doesn't help a model with no MLX build: our 122B runs on Ollama's **llama.cpp Metal** path (GGUF) — silently, with no indication — and that is also the path where the `mmproj`/vision loading caveat below lives.
**[re-verified 2026-07-28 through v0.32.5 — re-check on each Ollama version bump whether 122B MLX support has since landed]**

- **Quantization.**
  The likely-actual path (llama.cpp Metal / GGUF): the Ollama library build is **Q4_K_M ≈ 81 GB**, and Unsloth's dynamic **UD-Q4_K_XL (~70 GB)** is the recommended quality/size balance.
  mlx-community MLX conversions of the 122B now **exist** on Hugging Face (4-bit ≈ 69.6 GB, plus 5-bit and 8-bit) **[verified 2026-07-28]**, so a standalone-MLX fallback is possible in principle — at the cost of Ollama's native `format` endpoint (see the fallback list under the open serving risk).
  If Ollama's own MLX backend covers the 122B, `mlx-community/Qwen3.5-122B-A10B-MLX-4bit` (~10 % less memory and 15–30 % faster than GGUF at the same precision) becomes the preferred build.
  **[community]** Fit re-confirmed 2026-07-07 from multiple independent secondary sources: ~70 GB on disk at 4-bit (Unsloth ladder: 3-bit 60 / 6-bit 106 / 8-bit 132 / BF16 245 GB), ~**74 GB resident** (MoE weights unpack 10–15 % larger than the file), so weights + a 5–10 GB long-context KV cache + the ~4 GB quantized embedder ≈ **84–89 GB** — inside even the default ~96 GB macOS Metal wired limit on a 128 GB machine, with real headroom (`iogpu.wired_limit_mb` can raise the ceiling if the KV cache is pushed harder).
  **[community — mutually consistent, none primary]**
- **Throughput.**
  ~**65–79 tok/s** on a 128 GB Mac — strong for the size (only 10 B params activate per token) — but **path-dependent**: the MLX backend is materially faster than the llama.cpp Metal fallback the 122B currently uses, so treat this as an optimistic estimate until the serving path is pinned.
  **[community — verify on M5]**
- **Memory budget is a three-way split: model weights + KV cache + the resident embedder, all inside 128 GB.**
  The KV cache grows **linearly with context length**, so quant level and working-context size trade against each other — you cannot run both the highest quant *and* the full 262 K window.
  Budget the context you actually need (see [§Context window](#context-window)), not the max.
- **`OLLAMA_FLASH_ATTENTION=1`** cuts KV-cache memory **30–50 %** — set it.
  If you hit cache instability, `--cache-type-k bf16 --cache-type-v bf16` is the fallback.
  **[community]**
- **`OLLAMA_NUM_PARALLEL=1`** — pin a single inference slot.
  The suite is strictly single-stream: the global run slot serializes jobs, holdings run in sequence, and the per-holding calls (research → distillation → interpretation → action) are sequential, so extra slots buy no throughput.
  Left unset, Ollama auto-picks **1 or 4** from detected memory.
  A 4-slot pick multiplies the KV cache the memory budget above plans at one slot.
  It also round-robins requests across slots, so a call can miss the slot holding the previous prompt's cached prefix — defeating the within-pass research-turn prefix reuse.
  One slot costs no per-call context, since `num_ctx` is per-request either way, and pinning it holds the memory fit steady across an Ollama version bump that would otherwise move the auto-default.
- **Daemon launch** (both flags are daemon-side, set at `serve` start): `OLLAMA_FLASH_ATTENTION=1 OLLAMA_NUM_PARALLEL=1 ~/ollama/v0.32.5/ollama serve`.
  Confirm the effective slot count in the serve log's runner line (`--parallel N`) and the working context via the `CONTEXT` column of `ollama ps`.

### The `num_ctx` trap (critical)

Ollama now **auto-sizes** the default context from detected memory (current docs: < 24 GiB → 4 K, 24–48 GiB → 32 K, **≥ 48 GiB → 256 K**), so on our 128 GB M5 the default lands near **256 K** — close to the native max, *not* tiny.
That sounds safe but cuts the other way: a 256 K window pre-allocates a **huge KV cache** that competes with the model weights and the resident embedder for the 128 GB, and the auto-value depends on the version and detected memory.
Both extremes hurt — too small silently drops prompt content, too large starves memory.
**Front-truncation is confirmed live** (M5, v0.32.5, 2026-07-28): a marker test at `num_ctx` 2048 with a ~4.6 K-token prompt kept the end marker and dropped the start marker — and the model then **confidently hallucinated** an answer over the missing head rather than flagging the gap, so the failure is silent *and* misleading.

- **Always set `num_ctx` explicitly** in the adapter `options`, sized to *just* hold the full packet + thinking budget + output — not the 256 K auto-default.
  This is both a correctness rule (no silent truncation of the deterministic packet) and a memory rule (KV cache scales linearly with it).
- **One `num_ctx` per model, not per stage.**
  The scheduler reloads a resident runner whenever a request's load-time options — `num_ctx` included — differ from the loaded ones, `keep_alive` notwithstanding, so alternating context sizes on one model bounces the full 81 GB load at every switch.
  Per-stage contexts therefore apply per *model*: when the optional fast tier falls back to the reasoner (the documented default roster), distillation shares the interpretation context (wired 2026-08-01; surfaced by external review against the pinned v0.32.5 scheduler).
- Symptom of setting it too *low*: **gibberish output** (the card's own tell).
- Confirm the effective value at runtime via the `CONTEXT` column of `ollama ps`.

### Serving path [resolved live on M5 2026-07-28, v0.32.5]

The serving path is now **pinned by observation** — the pre-flight resolved what this section previously tracked as open risk:

- **The 122B loads and serves on Ollama's llama.cpp Metal path (GGUF).**
  The serve log shows `ggml_metal_init` picking `Apple M5 Max` and a `llama_server` runner — not MLX — with the model 100 % GPU-resident (81 GB); first load from disk ≈ 15 s.
  Ollama's fast **MLX backend still doesn't cover the 122B-A10B** (re-verified 2026-07-28 through v0.32.5), and the fallback is silent, exactly as predicted — the log is the only place the backend is visible.
- **The official-library pull is clean, confirmed live**: the library `qwen3.5:122b-a10b` (Q4_K_M, 81 GB) pulls, loads, and serves; the runner even detects the bundled vision projector and translates it (`handle_qwen35_like_clip: detected Ollama-format qwen35moe GGUF used as mmproj; translating`) — no ollama#14575 `unknown model architecture` failure.
  The historical failure mode (verified 2026-07-07) remains scoped to **imported** GGUFs via a dual-`FROM` Modelfile with an `mmproj` sidecar — still the path to never take.
- **The architecture is hybrid-attention, which shrinks the KV budget dramatically**: only 12 layers carry a KV cache (192 MiB at 8 K context, f16) plus a small fixed recurrent-state buffer (~149 MiB, 48 layers) — extrapolating linearly, even the full 262 K window costs only ~6 GB of KV, well under the earlier 5–10 GB planning band.
  **[observed live 2026-07-28; extrapolation, not measured at 262 K]**
- **Pin the Ollama version; treat upgrades as re-verification events, not routine bumps.**
  Ollama's 2026 Apple-Silicon record argues for it: v0.20.4 shipped x86_64-only MLX dylibs that broke every MLX model on Apple Silicon (a regression, with recurrences reported on 0.20.5/0.21.0), and MLX crashed outright on M5-generation Metal (a bf16 tensor mismatch) for weeks in April 2026 before dedicated `mlx_metal_v4` builds landed for macOS 26 + M5 — M5-specific breakage historically gets fixed weeks *after* it's reported.
  Smoke-test the exact pinned version with the 122B on M5 arrival, and re-run the schema-integrity check on every bump.
  **[verified 2026-07-07]**

**The pre-flight verified all four open items on the pinned v0.32.5 (2026-07-28):** (1) the 122B loads and serves — on **llama.cpp Metal**; (2) 122B MLX acceleration has **not** landed; (3) `format` genuinely constrains output (24/24 schema-valid, malformed schema rejected — the #14645 fix behaves); (4) thinking produces a reasoning trace when `format` is absent.
If a future GGUF path won't load, fallbacks are a llama.cpp-compatible build, a **standalone MLX server** (e.g. `mlx-lm` / LM Studio — at the cost of Ollama's native `format` endpoint), or the mlx-community 122B conversions noted above.
The adapter seam ([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam)) isolates endpoint + model id, so a serving-path change is configuration not code — **but a non-Ollama server would change the `format` mechanism**, so this is the risk to retire first.

## The resident embedder

`Qwen3-Embedding-4B` stays resident alongside the reasoner for the suite's vector memory, consuming a few GB of the 128 GB budget — account for it when choosing the reasoner's quant and context size (see the memory split above).
It implements the existing `Embedder` trait, so nothing else changes.

## M5 pre-flight checklist

- [x] **Serving** *(2026-07-28)*: the 122B loads & serves on v0.32.5 — backend = **llama.cpp Metal** (M5 Max, 100 % GPU), official-library pull clean, `mmproj` issue does not bite (see [§Serving path](#serving-path-resolved-live-on-m5-2026-07-28-v0325)).
- [x] **Schema integrity** *(2026-07-28)*: 24/24 `think:false` + `format` schema-valid, malformed schema rejected HTTP 400, `think:true` + `tools` + `format` 8/8 clean — the #14645 fix behaves on v0.32.5; non-thinking distillation unlocked on this version.
- [x] **Thinking** *(2026-07-28)*: reasoning trace populated with thinking-on and no `format` (≈16 K chars); two-step reason→distill produced a schema-valid object.
- [x] **`num_ctx`:** behavior verified *(2026-07-28: explicit `num_ctx` honored per `ollama ps` `CONTEXT`; front-truncation of over-long prompts confirmed live)*; **wired 2026-08-01** — every Portfolio model stage sets an explicit per-stage `num_ctx` through the adapter option profiles (interpretation 128 K, honoring the ≥ 128 K thinking-capability advice; distillation 32 K on a genuinely distinct fast tier, sharing the 128 K interpretation context on the same-model fallback path — see the one-`num_ctx`-per-model rule in [§The `num_ctx` trap](#the-num_ctx-trap-critical)), never the version-dependent auto-size.
  **Residency wired in the same slice:** every chat call and the local embed call send `keep_alive: -1` (stay-resident, per the roster default); the daemon-side `OLLAMA_KEEP_ALIVE` stays untouched and user-owned.
  Confirm live at the next run via the `CONTEXT` column of `ollama ps`.
- [x] **Long-context probe** *(2026-07-28)*: in-house RULER-style check run at the deployed quant — 35/35 (needles at 3 depths + multi-hop + aggregation) across seven sizes 6 K → **160.6 K actual prompt tokens** with zero degradation; the ~130–170 K conservative budget now has measured support to its midpoint (see [§Context window](#context-window) and [verification/2026-07-28-m5-preflight.md](verification/2026-07-28-m5-preflight.md)).
- [x] **Memory** *(2026-07-28)*: `OLLAMA_FLASH_ATTENTION=1` set on the daemon; at the **full native 262 K** `num_ctx` the 122B reports 87 GB (81 GB weights + ~6 GB KV — hybrid attention keeps KV tiny) with the system still 31 % free alongside the embedder — the fit holds with ~40 GB headroom at the theoretical worst case.
- [x] **Sampling:** per-mode settings wired *(2026-08-01: the vendor table's thinking-general and non-thinking-general rows ship as adapter option profiles selected per stage — interpretation thinking-general, distillation non-thinking-general; unit tests pin the exact rows and forbid greedy decoding)*.
- [x] **`think` reaches the wire** *(wired 2026-08-01: the adapter's `think` is tri-state — `Some(false)` always serializes a literal `"think": false`, `None` omits the field — and the distill stage sends `Some(false)`; a wire test guards the regression)*.
  *(The finding it closes — 2026-07-31, first live Portfolio run: `ChatWire` skipped serializing `think` when false, so the intended non-thinking distill stage rode Qwen's thinking-on default, ~45 min of that run. Evidence: [verification/2026-07-31-first-live-portfolio-run.md](verification/2026-07-31-first-live-portfolio-run.md) §F3.)*
- **Eval-stats caveat for any instrumentation** *(2026-07-31)*: on v0.32.5 the `/api/chat` terminal-chunk `eval_count` / `eval_duration` cover only the post-thinking content phase — the thinking phase rides inside `total_duration` unaccounted.
  Compute effective tok/s from accumulated output chars ÷ elapsed, never from the reported eval fields (a thinking-heavy call otherwise reads as ~90 s of "missing" time).
- [x] **Throughput** *(2026-07-28, llama.cpp Metal path)*: measured across the probe — prompt eval 933 tok/s at 6 K falling to ~154 tok/s at 113 K; decode 41 tok/s at 6 K falling to ~16 tok/s at 113 K.
  At realistic packet sizes (≤ 30 K) that is seconds-to-a-minute of prompt eval plus ~25–40 tok/s decode — a thinking-heavy per-holding call (~7 K generated tokens) lands around 4–6 minutes, acceptable for the on-demand, checkpoint/resume job design (the community 65–79 tok/s figure was indeed the MLX-path optimistic case).
  The adapter's transport-deadline floors (`DeadlinePolicy`, `local_model.rs`) are drafted just under the probe table's worst row (160.6 K: 113 tok/s prefill, 13.3 tok/s decode — [verification/2026-07-28-m5-preflight.md](verification/2026-07-28-m5-preflight.md)), so a serving-path change — an Ollama pin bump, an MLX backend — re-verifies them alongside the throughput itself.

## Sources

- [Hugging Face model card — Qwen/Qwen3.5-122B-A10B](https://huggingface.co/Qwen/Qwen3.5-122B-A10B)
- [Qwen documentation](https://qwen.readthedocs.io/en/latest/getting_started/quickstart.html)
- [Unsloth — Qwen3.5 run guide](https://unsloth.ai/docs/models/qwen3.5)
- [Ollama — MLX backend on Apple Silicon (v0.19)](https://ollama.com/blog/mlx)
- [Ollama — Structured outputs](https://docs.ollama.com/capabilities/structured-outputs) · [Ollama — Context length](https://docs.ollama.com/context-length)
- [Ollama JSON mode × thinking interaction (zenn)](https://zenn.dev/7shi/articles/fa36989a04c9ed?locale=en)
- [ollama/ollama #14645 — `format` ignored when `think` disabled (qwen3.5)](https://github.com/ollama/ollama/issues/14645) · [#15260 — same bug class, gemma4](https://github.com/ollama/ollama/issues/15260)
- [ollama/ollama PR #15901 — the #14645 fix (merged 2026-07-07, first shipped in v0.32.0)](https://github.com/ollama/ollama/pull/15901)
- [ollama/ollama #14575 — `qwen35moe` mmproj/GGUF-import loading failure (canonical, open)](https://github.com/ollama/ollama/issues/14575)
- [ollama/ollama #13820 — GLM XML tool-call parsing failures](https://github.com/ollama/ollama/issues/13820) · [#10222 — Go-template gaps vs Jinja](https://github.com/ollama/ollama/issues/10222)
- [Ollama releases — v0.30.5–v0.32.5 window checked for MLX rollout](https://github.com/ollama/ollama/releases) · [qwen3.5 library tags — `-mlx` coverage](https://ollama.com/library/qwen3.5/tags)
- [NVIDIA RULER — effective context benchmark](https://github.com/NVIDIA/RULER)
