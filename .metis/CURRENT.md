# Current session handoff

## What happened

**Vue SFC component-test harness shipped** — squash-merged to `main` @ `1872cfe`, pushed to `origin/main` (branch `vue-sfc-test-harness` deleted). This stands up the first behavioral coverage for Vue SFCs (previously the `vue-tsc` type-check was the only floor), realizing BUILD.md §Testing approach's "UI components get component tests." `npm test` is now **two runners split by file extension**: pure modules `tests/**/*.test.ts` via Node's built-in runner (type-stripped, unchanged — `renderChart`), and SFC tests `tests/**/*.spec.ts` via **Vitest** (`vitest.config.ts`: `@vitejs/plugin-vue` + happy-dom + `@vue/test-utils`). The Vitest `include` is **scoped to `*.spec.ts` on purpose** — its default glob would otherwise grab the `node:test` `*.test.ts` files. First spec `tests/components/ResearchDocuments.spec.ts` (5 tests) pins the prior session's inbox failed-row a11y contract that shipped untested: full-name `:title` tooltip, `aria-describedby` reason↔Delete pairing (+ healthy-row negative), parse-failed tag, two-step delete (confirm controls reached by label, not position). CLAUDE.md verification doctrine documents the split. Two external **Codex** rounds closed: (1) `engines` admitted EOL Node 23.x that Vitest 4 doesn't support; (2) the lockfile's root `engines` mirror was stale — both fixed by tightening `package.json` **and** `package-lock.json` to `>=22.18 <23 || >=24` (the intersection of "type-stripping works" — 22.18 is its floor — and "Vitest supports").

## Current state

On **`main` @ `1872cfe`**, synced with `origin/main`, **nothing in flight**. Frontend gate green: `npm run build` clean; `npm test` now two-runner — **node:test 38/0 + Vitest 5/0**. No Rust touched (backend unchanged: `cargo test` 333/0/14, clippy clean). No live API spend. The harness exists but covers **one** component; `invoke`-calling SFCs aren't yet testable (see Open questions).

## Open questions

- **Tauri mocking unbuilt** — `ResearchDocuments` needed none (it emits to its parent), but the next spec targeting `Settings.vue`/`App.vue` (which call `invoke()`) needs an `@tauri-apps/api` mock; the harness supports it, the pattern isn't established.
- **Pre-existing esbuild/vite advisory** — `npm audit` flags 3 high-sev esbuild issues, transitive through the existing `vite` (already in range); the only fix is a breaking vite 8 upgrade — parked, not introduced by the harness.
- **Per-bar emphasis** — out of scope; `emphasis` is series-level, needs a new per-point field.
- Recording the ` ```chart ` JSON syntax in `docs/report-structure.md` — still **optional**.
- **GPT-5-mini extraction stage** — conditional follow-on, only if users drop docs > ~12k chars; seam ready.
- *(carried)* Learning dedup unbuilt; Step-4 pull has no audit consumer; tuning bundle deferred (`MEMORY_TOP_K=5`, `LEARNINGS_PER_REPORT_CAP=5`, inbox caps, `COVERAGE_FLOOR=0.6` not final); `fmp_baseline_smoke` unrun since quota reset; tracker live-SSE smoke; wiremock / in-loop offline gap.
- *(low / parked)* FRED freshness tuning; calendar `expected` consensus; GDP not annualized; `cargo fmt` dirty repo-wide.

## Where to start

Nothing owed. The Vue SFC harness is in place. Three picks: **(a)** take a carried backend item — **learning dedup** or the **Step-4 vector-pull audit consumer** are most concrete; **(b)** extend component coverage to a second SFC, which would establish the **Tauri-mock pattern** for `invoke`-calling components; **(c)** the optional ` ```chart ` doc note is a low-effort filler.
