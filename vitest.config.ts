import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

// Component-test harness, kept separate from the Tauri dev config in
// `vite.config.ts` so the dev-server/HMR settings stay untouched. The `vue()`
// plugin compiles `.vue` SFCs exactly as in dev, so mounted components behave
// like the real thing.
//
// `include` is scoped to `*.spec.ts` on purpose: the pure-module tests under
// `tests/**/*.test.ts` (currently `renderChart.test.ts`) import `node:test` and
// run through Node's built-in runner with type-stripping — Vitest's default glob
// would also match `.test.ts` and try to run them, so the two runners are split
// by extension. Component (SFC) tests are `.spec.ts`; pure-module tests `.test.ts`.
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "happy-dom",
    globals: false,
    include: ["tests/**/*.spec.ts"],
  },
});
