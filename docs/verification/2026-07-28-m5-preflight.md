# M5 pre-flight verification — 2026-07-28

*The evidence record for the local-model serving pre-flight run on M5 arrival.
Conclusions are single-homed in [local-model-operations.md](../local-model-operations.md) (dated `[verified]` tags and the pre-flight checklist);
this file holds the measurements, methodology, and exact environment behind them.
The harness scripts are preserved in [scripts/](scripts/) so a future re-verification event
(any Ollama version bump, quant change, or serving-path change) can re-run the same checks and diff against these numbers.*

## Environment

- **Hardware:** MacBook Pro (`Mac17,7`), Apple M5 Max, 128 GB unified memory, macOS 26.5.1.
- **Ollama:** v0.32.5, the standalone `ollama-darwin.tgz` release build (sha256-verified against the release manifest),
  extracted to `~/ollama/v0.32.5/` — deliberately not the auto-updating `.app`, so the version is pinned.
- **Daemon:** `OLLAMA_FLASH_ATTENTION=1 ~/ollama/v0.32.5/ollama serve` (defaults otherwise; keep-alive at the stock 5 minutes).
- **Reasoner:** `qwen3.5:122b-a10b` — official library pull, `qwen35moe`, 125.1 B params, **Q4_K_M**, 81 GB on disk, 262,144 native context.
- **Embedder:** `qwen3-embedding:4b` — 2.5 GB; `/api/embed` returned 2560-dim vectors (correct for Qwen3-Embedding-4B).

## Serving & backend

- First load of the 122B: **≈ 15.6 s** from SSD (`load_duration` on the first chat call); generation correct from the first token.
- **Backend = llama.cpp Metal**, positively identified from the serve log: `ggml_metal_init` picking `Apple M5 Max`, a `llama_server` runner, model **100 % GPU-resident**.
  MLX does not cover the 122B on v0.32.5 (the bundle does ship `mlx_metal_v4` dylibs — the macOS 26 + M5 builds — so the MLX backend itself is healthy for covered models).
- **The `mmproj` caveat does not bite the official-library pull:** the runner detected the bundled vision projector and translated it
  (`handle_qwen35_like_clip: detected Ollama-format qwen35moe GGUF used as mmproj; translating`) — no ollama#14575 `unknown model architecture` failure.
- **Hybrid attention observed:** only 12 layers carry a KV cache (`llama_kv_cache: 192.00 MiB … 12 layers` at 8 K context, f16)
  plus a fixed ~149 MiB recurrent-state buffer across 48 layers (`llama_memory_recurrent`) — the reason long-context KV stays small (see Memory).

## Schema integrity (#14645) and thinking

All calls via `/api/chat` against the 122B, `num_ctx` 16,384, sampling per the ops doc's per-mode rows.
Schema: a 4-field object (`reasoning` / `ticker` / `verdict` enum / `confidence` 0–1), `additionalProperties: false`, validated app-side.

| Test | Configuration | Result |
|---|---|---|
| A | `think:false` + `format` (the historically bugged config) | **24/24 schema-valid** (8 + 16 confirmation reps) |
| B | `think:true` + `format` | schema-valid; `message.thinking` populated (≈ 18 K chars) |
| C | `think:true`, no `format` | reasoning trace populated (≈ 16 K chars); prose answer |
| D | two-step reason → distill (`format` + `think:true`, precise row) | schema-valid |
| E | structurally malformed `format` value | **rejected HTTP 400** — not silently passed through |
| F | `think:true` + `tools` + `format` (uncorroborated v0.20.2 report) | **8/8 schema-valid** — does not reproduce |

Pre-fix, test A failed ~1 in 3 calls; 24 consecutive clean reps ≈ 0.006 % pass probability were the bug live.
Verdict: **the #14645 fix behaves on v0.32.5; non-thinking distillation is unlocked on this version** (re-locks on any unverified bump).

## Truncation behavior

Marker test: `num_ctx` 2,048, prompt ≈ 4.6 K tokens with unique markers at the very start and very end.
Result: the end marker survived, the start marker was gone (`prompt_eval_count` 1,026) — **front-truncation confirmed** —
and the model **confidently hallucinated** an answer over the missing head rather than flagging the gap.
Silent and misleading; explicit per-stage `num_ctx` sizing is a correctness rule, not just a memory rule.

## Long-context probe (in-house RULER-style)

No published effective-context measurement exists for this model (surveyed 2026-07-07), so this is the first known one at this quant.

**Method** (`scripts/longctx_probe.py`): a deterministic synthetic financial dossier (seeded filler paragraphs of plausible market commentary) is built at each target size, with
three unique needle facts planted at 10 % / 50 % / 90 % depth,
a 3-link multi-hop chain scattered at 20 % / 55 % / 85 % (subsidiary → CFO → prior-employer metric),
and three aggregation terms scattered at 15 % / 45 % / 75 % (segment capex figures to sum).
All five questions are asked in one call per size (amortizing prompt eval); scoring is exact-value match on formatted answer lines;
sampling = the thinking-general row; `num_ctx` = estimated prompt × 1.15 + 40,960, capped at the 262,144 native window.

| Actual prompt tokens | `num_ctx` | Prompt eval | Decode | Needles 10/50/90 % | Multi-hop | Aggregation |
|---|---|---|---|---|---|---|
| 5,913 | 50,160 | 6.3 s (933 tok/s) | 6,718 tok @ 41.1 tok/s | 3/3 | PASS | PASS |
| 22,697 | 77,760 | 57.3 s (396 tok/s) | 7,276 tok @ 27.1 tok/s | 3/3 | PASS | PASS |
| 45,147 | 114,560 | 139.9 s (323 tok/s) | 10,098 tok @ 23.8 tok/s | 3/3 | PASS | PASS |
| 67,535 | 151,359 | 284.5 s (237 tok/s) | 7,618 tok @ 20.3 tok/s | 3/3 | PASS | PASS |
| 90,011 | 188,160 | 486.4 s (185 tok/s) | 8,885 tok @ 17.8 tok/s | 3/3 | PASS | PASS |
| 112,708 | 224,960 | 733.7 s (154 tok/s) | 5,578 tok @ 16.3 tok/s | 3/3 | PASS | PASS |
| 160,589 | 262,144 | 1,425.7 s (113 tok/s) | 8,199 tok @ 13.3 tok/s | 3/3 | PASS | PASS |

**Verdict: 35/35 — zero degradation through 160,589 prompt tokens (61 % of the native window).**
Scope honesty: this measures retrieval, chaining, and aggregation — not the full synthesis workload —
so the ops doc keeps the ~130–170 K conservative budget as the planning ceiling, now supported by measurement to its midpoint.

## Memory

- 8 K context: 81 GB reported resident.
- **Full native 262,144 context: 87 GB reported** (weights + ≈ 6 GB KV — the hybrid-attention extrapolation held), `llama-server` RSS 83.6 GB,
  system-wide free memory **31 %** (≈ 40 GB) with the daemon and everything else loaded.
- The weights + KV + embedder three-way fit holds with large headroom even at the theoretical worst case; no wired-limit tuning needed.

## Throughput

From the probe table: prompt eval falls 933 → 113 tok/s and decode 41 → 13 tok/s across 6 K → 160.6 K.
At the suite's realistic packet sizes (≤ 30 K): prompt eval is seconds-to-a-minute and decode ≈ 25–40 tok/s,
so a thinking-heavy per-holding call (≈ 7 K generated tokens) lands around **4–6 minutes** —
acceptable for the on-demand, checkpoint/resume job design.
The community 65–79 tok/s figure is confirmed as the MLX-path optimistic case, not this llama.cpp-path reality.

## Findings — outstanding work surfaced by the pre-flight

1. **The adapter never sets `options`** (`ChatRequest::new` defaults it to `None`; no pipeline call site populates it):
   no explicit `num_ctx` (calls ride the version-dependent auto-size, ~256 K on this machine),
   no per-mode sampling rows (Modelfile defaults only — non-greedy, but no `presence_penalty` and no precise-row distillation),
   and no `keep_alive` (the documented stay-resident roster posture is unimplemented; cost today is a ~15 s reload after 5 idle minutes).
   One small named code slice covers all three; recorded on the ops-doc checklist (`num_ctx` and Sampling boxes).
2. **Daemon supervision is session-scoped** — `ollama serve` was started by hand; nothing restarts it at login.
   Already owned by the named guided-setup follow-up (BUILD §What remains).

## Repro commands

```bash
# Install (pinned)
curl -sSL -o ollama-darwin.tgz https://github.com/ollama/ollama/releases/download/v0.32.5/ollama-darwin.tgz
curl -sSL -o sha256sum.txt https://github.com/ollama/ollama/releases/download/v0.32.5/sha256sum.txt
shasum -a 256 -c <(grep ollama-darwin.tgz sha256sum.txt)
mkdir -p ~/ollama/v0.32.5 && tar -xzf ollama-darwin.tgz -C ~/ollama/v0.32.5
ln -sf ~/ollama/v0.32.5/ollama ~/.local/bin/ollama

# Serve + models
OLLAMA_FLASH_ATTENTION=1 ~/ollama/v0.32.5/ollama serve &
ollama pull qwen3.5:122b-a10b
ollama pull qwen3-embedding:4b

# Checks
python3 scripts/preflight_schema.py 8     # schema-integrity matrix (A–F)
python3 scripts/longctx_probe.py 8 32 64 96 128 160 228   # long-context probe legs
```
