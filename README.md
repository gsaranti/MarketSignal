# Market Signal

A local-first desktop application that generates an evolving, professional **Weekly Market Report** rather than reactive daily commentary.

Market Signal is not a trading bot. It is a market-analysis and thesis-generation system: it runs a single recurring weekly job and presents an evolving market thesis — covering market regimes, macro developments, geopolitical and economic events, sector analysis, and forward-looking preparation. The weekly cadence is intentional, prioritizing signal over noise and thesis continuity over daily reaction. Everything runs on your machine except external API and model requests.

## Tech stack

- **[Tauri](https://tauri.app/) 2** — desktop shell with a Rust backend and a system webview
- **[Vue](https://vuejs.org/) 3** + **TypeScript** — frontend (`<script setup>` SFCs)
- **[Vite](https://vite.dev/)** — frontend build and dev server
- **Rust** — backend, with the frontend communicating via Tauri `invoke()`

## Prerequisites

- [Node.js](https://nodejs.org/) (with `npm`) — any recent version builds and runs the app; **`npm test` needs Node 22.18+ or 24+** (not the EOL Node 23 line): it runs the pure-module suite directly via TypeScript type-stripping and the Vue component suite via Vitest
- A [Rust toolchain](https://www.rust-lang.org/tools/install) (stable)
- The platform-specific [Tauri system dependencies](https://tauri.app/start/prerequisites/)

## Getting started

Install dependencies:

```bash
npm install
```

Run the app in development:

```bash
npm run tauri dev
```

Build a production bundle:

```bash
npm run tauri build
```

### Demo mode (no API keys)

To exercise the full report flow without any API keys, network calls, or cost:

```bash
npm run tauri:demo
```

Clicking **Generate now** drives the real report pipeline against built-in stubs — streaming the run tracker and rendering a complete (stub) report. It's a dev-only `demo-run` Cargo feature, excluded from production builds (`npm run tauri build`), and is the intended way to work on the run tracker, report rendering, and other UI without spending data-provider or model quota.

## Development

Run the checks before committing a change:

```bash
npm run build                                 # frontend: vue-tsc type-check + Vite build
npm test                                       # frontend unit tests — type-stripping + Vitest (Node 22.18+/24+)
cd src-tauri && cargo test                    # backend tests
cargo clippy --all-targets --all-features     # backend lint — kept warning-free
```

## Documentation

The product and architecture are specified in [`docs/`](docs/), indexed by [`docs/README.md`](docs/README.md).

UI work is governed by the design system in [`market-signal-design-system/`](market-signal-design-system/), the source of truth for tokens, components, voice, and layout.

## License

[MIT](LICENSE) © 2026 George Sarantinos
