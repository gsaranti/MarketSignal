# Local Model Operational Reference — Qwen3.5-122B-A10B

Operational best-practices for the local suite's primary reasoner. This is the
**how to run it well** companion to [local-models.md](local-models.md) (which
covers the *architecture* — the serving runtime, roster, adapter seam, and
context-memory discipline). Everything here concerns the one model the roster
defaults to for every reasoning role: **`Qwen3.5-122B-A10B`**.

**Status — research-derived, not yet live-validated.** Every figure below is from
vendor documentation (the Hugging Face model card, Qwen docs) or community
runtime sources, gathered while the project is hardware-gated on the M5 (the M1
dev machine can't host the model). Claims are tagged **[vendor]** (documented by
Qwen / a model card), **[community]** (runtime reports, treat as directional), or
**[verify on M5]** (a live pre-flight check, not yet confirmed). Re-validate the
**[verify on M5]** items before the first real local run.

## Why this model (compact)

`Qwen3.5-122B-A10B` is the default because it is the open-weight model that best
satisfies the suite's hard constraints simultaneously: **open / keyless**
(Apache 2.0), **fits a 128 GB Apple-Silicon machine** with the embedder
co-resident, a **262 K context window**, a real **thinking mode**, and — the
load-bearing property for a schema-validated pipeline — **reliable
grammar-constrained JSON** on the Qwen family (the alternatives that fit either
gave up structured-output reliability on Ollama or didn't fit cleanly). This
choice is worth revisiting after v2 ships; it is not permanent.

## Model at a glance [vendor]

- **Released** ~Feb 2026, **Apache 2.0**.
- **Sparse MoE:** 122 B total parameters, **10 B activated** per token; **256
  experts** (8 routed + 1 shared); 48 layers, hidden dim 3072.
- **Hybrid attention:** Gated DeltaNet (linear) + Gated Attention (full) layers
  in roughly a 3:1 ratio — this is *why* it carries long context cheaply, and
  also why runtime support is newer/less universal than a plain-transformer model
  (see [§Serving](#serving--memory-apple-silicon-128-gb)).
- **Natively multimodal** (the 3.5/3.6 generation ships a vision encoder). **We
  use it text-only.** This matters operationally only because the multimodal
  packaging (a separate `mmproj` vision projector in GGUF builds) is the source of
  the Ollama loading caveat below.

## Context window

- **262,144 tokens native**, extensible to ~**1.01 M** via YaRN (RoPE scaling).
  **[vendor]**
- **We do not need YaRN.** The suite's context-memory discipline assembles compact,
  bounded per-item packets ([local-models.md §Context-memory discipline](local-models.md#context-memory-discipline));
  262 K native is far more than any packet uses, so the model runs at native scale
  and YaRN extension stays off (it costs accuracy at shorter lengths).
- **Effective context is well below the advertised window.** There is no published
  RULER curve for *this* model, so treat this as a **planning heuristic, not a
  measured property**: across long-context models generally, effective context
  often lands around **~50–65 % of the stated window** — they degrade on multi-hop
  reasoning and aggregation (our synthesis workload) long before the hard limit,
  even while acing simple needle-in-a-haystack retrieval. As a **conservative
  budget**, plan reliable use up to ~130–170 K and treat beyond as degrading rather
  than failing. **[community / researched judgment — not a vendor number; validate
  with a Qwen3.5-specific RULER/LongBench figure or a live run on the M5]**
- **Do not starve the window either.** The model card advises keeping context
  **≥ 128 K to preserve thinking capability** — thinking chains are long
  (tens of thousands of tokens), so a too-small window truncates reasoning. The
  practical target is a *generous* context that comfortably holds packet + thinking
  + output, not the smallest that fits. **[vendor]**

## Thinking mode

- **On by default**, emitting a `<think>…</think>` block before the answer.
  **[vendor]**
- **Disable per call** with `chat_template_kwargs: {"enable_thinking": false}`
  (on Ollama, the `think` parameter / `think: false`). **[vendor]**
- **No `/think` · `/nothink` soft switch.** This is a deliberate change from
  Qwen3 — 3.5 supports only the hard `enable_thinking` flag, so mode is chosen by
  the call, never by an inline token in the prompt. Our adapter already selects
  mode per stage, so this is a non-issue *as long as no stage relies on the soft
  switch.* **[vendor]**
- **Strip thinking from history.** In a multi-turn loop, exclude prior
  `<think>` blocks from the messages sent on later turns — the card is explicit
  that thinking content must not accumulate in conversation history. **[vendor]**
- **Generation length:** 32,768 tokens for most queries; up to 81,920 for the
  hardest multi-step problems. Size `num_ctx` to hold input + thinking + this. **[vendor]**

## Structured output × thinking — the one incompatibility that bites us

This is the single most load-bearing operational fact for our pipeline, because
the suite's entire stage-to-stage contract is grammar-constrained schema-valid
JSON via Ollama's `format` ([local-models.md §Schema-constrained output](local-models.md#schema-constrained-output)),
and the model defaults to thinking-on. The mechanic is asymmetric: Ollama applies
the `format` GBNF grammar mask **only after the end-of-thinking token**, so the
two flags behave very differently together.

- **`think: true` + `format` *composes* — this is the safe path.** The model
  produces its reasoning, closes the thinking block, and the grammar then
  constrains the final answer to schema-valid JSON. Reasoning lands in the separate
  `message.thinking` field, the schema-valid object in `content` — you get *both*
  reasoning and a constrained schema in one call. **[community]**
- **`think: false` + `format` is BROKEN** — bug #14645, *"format is ignored when
  think is disabled for qwen3.5 series."* With thinking off, the end-of-thinking
  token the mask waits for is never emitted, so `format` never engages and the
  model returns **free-form text where you asked for schema-valid JSON** — the exact
  "parse-and-pray" failure the suite forbids, and *silent*. The trap: the intuitive
  "fast, non-thinking distill" call (`think: false` + `format`) is precisely the
  bugged configuration — and it is also how [local-models.md](local-models.md)'s
  "non-thinking distillation" mode would naively be wired. **[verify on M5 — confirm
  whether #14645 is fixed on our Ollama version]**
- **Our rule, until #14645 is confirmed fixed on our version:** every call that
  carries `format` **keeps thinking enabled** (`think: true` + `format`, which
  works — accept the extra thinking tokens), or the schema is validated app-side
  instead of trusting the grammar. **Never ship an unverified `think: false` +
  `format` call.** Two patterns fit:
  1. **Two-step (heavy stages).** A thinking call reasons freely (no `format`),
     then a **second `format`-carrying call — thinking still on** — distills into
     the schema object. This is the suite's research/interpretation →
     schema-distillation split; the only thing the bug changes is that the distill
     call stays thinking-on rather than running non-thinking.
  2. **Reasoning-field-first (light stages).** For a stage wanting a little
     reasoning *and* structure in one call, put a `reasoning` string field
     **first** in the schema (`{"reasoning": "...", ...}`) so the model reasons
     into that field before the structured fields — naturally a thinking-on call.

## Sampling settings [vendor]

Set these per call via the adapter `options`, switched by mode. Greedy decoding
is **explicitly warned against** — temperature 0 / disabled sampling drives the
model into repetition loops and quality collapse.

| Mode | temperature | top_p | top_k | min_p | presence_penalty |
|---|---|---|---|---|---|
| Thinking — general | 1.0 | 0.95 | 20 | 0.0 | 1.5 |
| Thinking — precise/coding | 0.6 | 0.95 | 20 | 0.0 | 0.0 |
| Non-thinking — general | 0.7 | 0.8 | 20 | 0.0 | 1.5 |
| Non-thinking — reasoning | 1.0 | 1.0 | 40 | 0.0 | 2.0 |

- `presence_penalty` may be tuned 0–2 to curb repetition; **too high causes
  language-mixing and quality loss.**
- Default mapping for our stages: **research / interpretation → thinking-general**;
  **schema distillation → a `format`-carrying call with thinking *enabled*** (the
  #14645 constraint above — *not* non-thinking — until the bug is verified fixed on
  our version; use the lower-temperature **thinking — precise/coding** row for a
  firm, deterministic distillation).

## Serving & memory (Apple Silicon, 128 GB)

The suite serves through **Ollama** ([local-models.md §Serving runtime](local-models.md#serving-runtime)).
Ollama added a genuine **MLX backend** on Apple Silicon in **v0.19** (Mar 2026;
requires ≥32 GB unified memory, else it falls back to llama.cpp Metal). The
caveat that lands directly on our choice: **the MLX backend currently accelerates
only select architectures — at time of writing, Qwen3.5-*35B*-A3B, not the
122B-A10B.** So our 122B almost certainly runs today on Ollama's **llama.cpp
Metal** path (GGUF), *not* MLX — silently, with no indication — and that is also
the path where the `mmproj`/vision loading caveat below lives. **[community —
verify on M5 whether 122B MLX acceleration has since landed]**

- **Quantization.** The likely-actual path (llama.cpp Metal / GGUF): the Ollama
  library build is **Q4_K_M ≈ 81 GB**, and Unsloth's dynamic **UD-Q4_K_XL (~70 GB)**
  is the recommended quality/size balance. If/when the 122B becomes
  MLX-accelerated, `mlx-community/Qwen3.5-122B-A10B-MLX-4bit` (~70–75 GB, ~10 %
  less memory and 15–30 % faster than GGUF at the same precision) becomes the
  preferred build. **[community]**
- **Throughput.** ~**65–79 tok/s** on a 128 GB Mac — strong for the size (only
  10 B params activate per token) — but **path-dependent**: the MLX backend is
  materially faster than the llama.cpp Metal fallback the 122B currently uses, so
  treat this as an optimistic estimate until the serving path is pinned. **[community
  — verify on M5]**
- **Memory budget is a three-way split: model weights + KV cache + the resident
  embedder, all inside 128 GB.** The KV cache grows **linearly with context
  length**, so quant level and working-context size trade against each other —
  you cannot run both the highest quant *and* the full 262 K window. Budget the
  context you actually need (see [§Context window](#context-window)), not the max.
- **`OLLAMA_FLASH_ATTENTION=1`** cuts KV-cache memory **30–50 %** — set it.
  If you hit cache instability, `--cache-type-k bf16 --cache-type-v bf16` is the
  fallback. **[community]**

### The `num_ctx` trap (critical)

Ollama now **auto-sizes** the default context from detected memory (current docs:
< 24 GiB → 4 K, 24–48 GiB → 32 K, **≥ 48 GiB → 256 K**), so on our 128 GB M5 the
default lands near **256 K** — close to the native max, *not* tiny. That sounds
safe but cuts the other way: a 256 K window pre-allocates a **huge KV cache** that
competes with the model weights and the resident embedder for the 128 GB, and the
auto-value depends on the version and detected memory. Both extremes hurt — too
small silently drops prompt content (over-long prompts are reported to be
truncated, commonly from the front — **verify the exact behavior, don't assume
it**), too large starves memory.

- **Always set `num_ctx` explicitly** in the adapter `options`, sized to *just*
  hold the full packet + thinking budget + output — not the 256 K auto-default.
  This is both a correctness rule (no silent truncation of the deterministic
  packet) and a memory rule (KV cache scales linearly with it).
- Symptom of setting it too *low*: **gibberish output** (the card's own tell).
- Confirm the effective value at runtime via the `CONTEXT` column of `ollama ps`.

### Open serving risk [verify on M5]

The serving path for the 122B is **not yet pinned** and must be resolved live
before the first run:

- Ollama's fast **MLX backend currently covers only select architectures (the
  35B-A3B, not the 122B-A10B)**, so the 122B falls back to Ollama's **llama.cpp
  Metal** path (GGUF) today — silently, with no indication.
- On that GGUF path, community runtime docs report **Qwen3.5 GGUFs failing to
  load in Ollama** because the multimodal build ships a separate `mmproj` vision
  file the loader mishandles. Yet an `ollama.com/library/qwen3.5:122b-a10b` entry
  exists at Q4_K_M — so either it's been resolved or the library entry is
  unreliable; we can't tell from the outside.

**Pre-flight on the M5 must verify, on the exact Ollama version we ship:** (1) the
122B actually loads and serves text generation — and *which* backend it lands on
(MLX vs llama.cpp Metal); (2) whether 122B MLX acceleration has since landed;
(3) `format` *actually* constrains output to the schema (not bug #14645);
(4) thinking produces a reasoning trace when `format` is absent. If the GGUF path
won't load, fallbacks are a llama.cpp-compatible build, a **standalone MLX server**
(e.g. `mlx-lm` / LM Studio — at the cost of Ollama's native `format` endpoint), or
waiting on 122B MLX support. The adapter seam
([local-models.md §The local-model adapter seam](local-models.md#the-local-model-adapter-seam))
isolates endpoint + model id, so a serving-path change is configuration not code —
**but a non-Ollama server would change the `format` mechanism**, so this is the
risk to retire first.

## The resident embedder

`Qwen3-Embedding-4B` stays resident alongside the reasoner for the suite's
vector memory, consuming a few GB of the 128 GB budget — account for it when
choosing the reasoner's quant and context size (see the memory split above). It
implements the existing `Embedder` trait, so nothing else changes.

## M5 pre-flight checklist

- [ ] **Serving:** the 122B loads & serves text generation on our Ollama version,
  and we know *which* backend it lands on (MLX vs llama.cpp Metal) and whether the
  GGUF/`mmproj` issue bites (resolves the [open serving risk](#open-serving-risk-verify-on-m5)).
- [ ] **Schema integrity:** `format` genuinely constrains output (not bug #14645);
  a malformed-schema attempt is rejected, not silently passed through.
- [ ] **Thinking:** reasoning trace appears with thinking-on and *no* `format`;
  the two-step reason→distill pattern produces schema-valid objects.
- [ ] **`num_ctx`:** set explicitly; confirm a max-size packet is **not**
  front-truncated (check `ollama ps` `CONTEXT`).
- [ ] **Memory:** `OLLAMA_FLASH_ATTENTION=1`; model + KV cache (at chosen context)
  + embedder fit 128 GB with headroom.
- [ ] **Sampling:** per-mode settings wired; **no greedy decoding** anywhere.
- [ ] **Throughput:** measure real tok/s at our context size; confirm acceptable
  wall-clock for a full per-item loop.

## Sources

- [Hugging Face model card — Qwen/Qwen3.5-122B-A10B](https://huggingface.co/Qwen/Qwen3.5-122B-A10B)
- [Qwen documentation](https://qwen.readthedocs.io/en/latest/getting_started/quickstart.html)
- [Unsloth — Qwen3.5 run guide](https://unsloth.ai/docs/models/qwen3.5)
- [Ollama — MLX backend on Apple Silicon (v0.19)](https://ollama.com/blog/mlx)
- [Ollama — Structured outputs](https://docs.ollama.com/capabilities/structured-outputs)
  · [Ollama — Context length](https://docs.ollama.com/context-length)
- [Ollama JSON mode × thinking interaction (zenn)](https://zenn.dev/7shi/articles/fa36989a04c9ed?locale=en)
- [ollama/ollama #14645 — `format` ignored when `think` disabled (qwen3.5)](https://github.com/ollama/ollama/issues/14645) · [#15260 — same bug class, gemma4](https://github.com/ollama/ollama/issues/15260)
- [NVIDIA RULER — effective context benchmark](https://github.com/NVIDIA/RULER)
