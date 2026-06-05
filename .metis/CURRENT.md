# Current session handoff

## What happened

**Dark mode shipped — toggle + two AA contrast fixes — merged and pushed to `origin/main`** (3 commits, `bc4fab7..90a68d4`). 

- **Dark-mode toggle** (`3a5349e`): a **Light/Dark boolean**, kit-faithful ("Dark surface"), default Light — *not* tri-state/System (decided: not App-Store-required, simpler, explicit opt-in). New `src/theme.ts` is the **only** setter of `data-theme` on `<html>`; persisted in **localStorage, not `app_settings`** (pure presentation, no backend consumer; synchronous read in `main.ts` before mount → no flash). Applies instantly, independent of the token-gated Save. `color-scheme` declared light/dark so native chrome matches.
- Making the dark palette user-facing exposed two **pre-existing, latent** token-contrast gaps (caught by three converging reviews — metis reviewer + Codex ×2):
  - **`--ink-3`** retuned (`fd7238c`): light `#7A6F5F→#6B6153`, dark `#7E7560→#988F78` — clears AA 4.5:1 on paper/soft/edge in both modes; ramp hierarchy preserved.
  - **`--accent` as text** decoupled (`90a68d4`): new **`--accent-text`** token (dark `#D28A99`; light = `--accent`, unchanged). 8 `color:` text sites + warning chevron repointed; fills/rings/`::selection` keep `--accent` (dark `#B0596A` fails AA as text **by design**).
- Verified throughout: contrast computation (all six surface/mode pairs), `cargo test`/clippy/`npm run build`, and live light+dark GUI screenshots (non-destructive, real DB backed up/restored).

## Current state

On **`main`** at **`90a68d4`**, **pushed** (up to date with `origin/main`), working tree clean, `dark-mode` branch merged + deleted. **Nothing in flight** — the feature is complete.

## Open questions

- **Appearance persistence is localStorage, not `app_settings`** — a deliberate departure from the "persisted config lives in SQLite" convention. Justified (UI preference, not gated config; avoids launch flash). Flagged below for whether `BUILD.md` should note it.
- **`--accent-text` dark = `#D28A99`** — comfortable AA margin but a notable pink shift from the oxblood; could dial toward `#CE8090` (~4.95) if it reads too pink. Accepted as-is; low priority.
- **No Vue component-test harness** *(carried)* — the toggle's disabled/on/off matrix is covered by build + live screenshot, not Vitest. Stand up Vitest + Vue Test Utils when wanted.
- *(carried)* Test-saved vs test-before-save (Settings); FMP `200`-with-error-body branch unverified live; **Jun-3 failed job** in the real DB (uninvestigated); retention-cascade enforcement; step-5 auto-archive; report-body fidelity ceiling; PDF `@page` margin fidelity.

*(Resolved this session: the `--ink-3` caption AA gap and the latent dark-mode contrast concern — both fixed and verified.)*

## Where to start

Dark mode is **done + pushed**; not a candidate anymore. Pick the next build target → `/metis-plan-task`: per `BUILD.md`'s "immediate next slices," the **FMP/FRED/BLS data-source adapters** are the next major build; **retention-cascade enforcement** is a smaller self-contained one. The **Jun-3 failed job** in the DB remains a cheap aside if curious.
