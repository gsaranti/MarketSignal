# Current session handoff

## What happened

**Step-4 audit-memory shipped** — planned, implemented, reviewed (`approve`), Codex-reviewed across two rounds, squash-merged to **`main` @ `65811e1`**, pushed to `origin/main`, branch `step4-audit-memory` deleted. This closes the carried "Step-4 vector-pull has no audit consumer" item. The Step-4 pre-research pull (previously routing-only, then dropped) now **also reaches the main agent's retrospective audit** on a new top-level `MainAgentInput.audit_memory` channel — a sibling of `baseline`/`deltas`, deliberately **not** inside the packet, so the packet still carries only the Step-10 research-informed pull (replace-not-merge honored). `assemble_research_packet` now returns `AssembledResearch { packet, audit_memory }` (the pull is cloned into routing, the original surfaced for the audit); the model adapter renders it as a prompt block distinct from the Step-10 memory.

**Design call worth carrying (Codex-driven, conceded on the merits):** the Retrospective Audit section is **gated on prior-summary presence** — `[summary · …]` fragments are the auditable object; `[learning · …]` fragments steer *what* to scrutinise and feed the thesis/strategy but do **not**, alone, license the section. I first argued a learnings-only recall could support a narrower audit; the docs define the section's object as prior *reports*, so the summary-gate is the right contract. **The gate is prompt-level (soft), not structural** — because the main agent has no separate Step-2 prior-report-context channel, `audit_memory` is its only summary signal.

## Current state

On **`main` @ `65811e1`**, synced with `origin/main`, **nothing in flight**. Backend gate green: `cargo test` **342/0/14**, `cargo clippy --all-targets --all-features` clean. No frontend touched, no live API spend. Four files changed (`agent.rs`, `pipeline.rs`, `model_agent.rs`, `tests/generate_report.rs`). Retrieval stays kind-blind (no per-kind quota) so learnings keep steering — only the prompt gates the section.

## Open questions

- **BUILD.md stale on this slice** — its storage/vector-memory section still calls "giving the Step-4 pull an audit consumer" the remaining follow-on; that shipped. Candidate BUILD.md update (user-run): record the audit consumer + the summary-presence gate and its soft/prompt-level nature. (See *Pending decisions*.)
- **Audit summary-gate is soft** — firms up when the **Step-2 main-agent prior-report-context slice** is built (the main agent has no recent-report channel today, only memory pulls); summary-presence could then become a structural input rather than prompt prose. Most concrete forward pointer.
- **`LEARNING_DEDUP_THRESHOLD` (0.93) unvalidated** against real `text-embedding-3-large` geometry — joins the tuning bundle.
- **`StubEmbedder` unfit for cosine-threshold tests** — consider promoting one shared separating double (`BasisEmbedder` / `DistinctEmbedder`).
- **Tauri mocking unbuilt** — the next SFC spec (`Settings.vue`/`App.vue`, which call `invoke()`) needs an `@tauri-apps/api` mock; pattern not established.
- **Pre-existing esbuild/vite advisory** — 3 high-sev, transitive through `vite`, only fix is a breaking vite 8 bump — parked.
- *(carried)* tuning bundle deferred (`MEMORY_TOP_K=5`, `LEARNINGS_PER_REPORT_CAP=5`, `LEARNING_DEDUP_THRESHOLD=0.93`, inbox caps, `COVERAGE_FLOOR=0.6`); `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; wiremock / in-loop offline gap; GPT-5-mini extraction stage (conditional, only if oversized inbox docs prove common); ` ```chart ` JSON-syntax doc note (optional); per-bar emphasis out of scope.
- *(low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide.

## Where to start

Nothing owed. Most concrete remaining: the **Step-2 main-agent prior-report-context slice** — gives the main agent the recent Markdown/summary context the audit is meant to evaluate, and would firm the audit's summary-gate from prompt-soft to structural. Alternatives: a **second SFC** spec to establish the Tauri-mock pattern for `invoke`-calling components; or the low-effort ` ```chart ` doc note. Consider a BUILD.md refresh first (see Pending decisions).
