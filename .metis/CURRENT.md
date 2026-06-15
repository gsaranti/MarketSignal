# Current session handoff

## What happened

**Learning dedup shipped** — planned, implemented, reviewed (`approve-with-nits`, both nits closed), squash-merged to `main` @ `80f4f9f`, pushed to `origin/main`, branch `learning-dedup` deleted. This closes the carried "learning dedup (unbuilt)" follow-on. The Step-17 persist now drops a durable learning whose embedding is within **`LEARNING_DEDUP_THRESHOLD` (0.93)** cosine of an existing `learning` row, reusing the embedding already computed (no extra paid call); within-run **and** cross-run restatements collapse because each insert is visible to the next scan. New `vector_memory::nearest_learning_similarity` (top-1 cosine over `learning` rows, kind-filtered, `None` when none can participate). The inline Step-17 learning-persist loop was **extracted into `pipeline::persist_durable_learnings`** as a unit-test seam — trim/cap, cancellation poll, provenance (`report_id`/`created_at`), and fail-soft posture unchanged; a dedup-scan failure **fails open** (keeps the learning, never loses one).

**Disproven-approach note worth carrying:** the `StubEmbedder` maps *distinct* prose to ~0.996 cosine (constant `v[0]=1.0` common-mode + positive byte increments), so it cannot drive any cosine-threshold test. Two pre-existing `tests/generate_report.rs` integration tests broke under dedup; fixed with a separating `DistinctEmbedder` (FNV→one-hot), **not** by lowering the threshold to dodge the stub.

## Current state

On **`main` @ `80f4f9f`**, synced with `origin/main`, **nothing in flight**. Backend gate green: `cargo test` **338/0/14** (lib 309 + integration), `cargo clippy --all-targets --all-features` clean. No frontend touched, no live API spend. Three files changed (`pipeline.rs`, `vector_memory.rs`, `tests/generate_report.rs`).

## Open questions

- **`LEARNING_DEDUP_THRESHOLD` (0.93) unvalidated** — conservative but not checked against real `text-embedding-3-large` geometry; related-but-distinct financial learnings can sit at high cosine, so 0.93 may prove too aggressive or too loose. One constant to retune once real learnings accumulate; joins the tuning bundle below.
- **`StubEmbedder` unfit for cosine-threshold tests** — any future similarity-gated feature needs a separating test embedder. The patterns now exist (`BasisEmbedder` in the pipeline unit tests, `DistinctEmbedder` in the integration crate); consider promoting one shared double.
- **Step-4 vector-pull has no audit consumer** — it feeds routing, not yet the Step-5 retrospective audit. Most concrete remaining backend item.
- **Tauri mocking unbuilt** — the next SFC spec targeting `Settings.vue`/`App.vue` (which call `invoke()`) needs an `@tauri-apps/api` mock; harness supports it, pattern not established.
- **Pre-existing esbuild/vite advisory** — 3 high-sev, transitive through `vite`, only fix is a breaking vite 8 bump — parked.
- Per-bar emphasis out of scope (needs a per-point field); recording the ` ```chart ` JSON syntax in `docs/report-structure.md` still optional.
- **GPT-5-mini extraction stage** — conditional follow-on, only if users drop docs > ~12k chars; seam ready.
- *(carried)* tuning bundle deferred (`MEMORY_TOP_K=5`, `LEARNINGS_PER_REPORT_CAP=5`, now `LEARNING_DEDUP_THRESHOLD=0.93`, inbox caps, `COVERAGE_FLOOR=0.6` not final); `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide.

## Where to start

Nothing owed. Most concrete remaining backend pick: the **Step-4 vector-pull audit consumer** (feeds routing, not yet the Step-5 audit). Alternatives: extend component coverage to a **second SFC** to establish the Tauri-mock pattern for `invoke`-calling components; or the low-effort ` ```chart ` doc note.
