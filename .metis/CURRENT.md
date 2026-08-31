# Current session handoff

## What happened

The big-run **launch session** — but mid-bring-up the user judged the
keyless-SearXNG strategy too fragile against Google's IP blocking (at bring-up
`google cse` + `duckduckgo` were dead from the first query, residual from the
2026-08-30 attempt) and chose to add a **Serper.dev paid SERP** as the reliable
floor (Finding 1's ranked mitigation #3). Serper is wired as pure SearXNG
`json_engine` config — app unchanged, suite stays SearXNG-only; it queries Google
from Serper's own infra (immune to the egress-IP blocks) and fires every query,
with the keyless engines kept as zero-cost bonus redundancy. The keyless set was
also **widened** (google/bing/qwant re-enabled) — the block set flips day to day,
so re-probe at every bring-up. Secret hygiene: `settings.yml` holds only a
`${SERPER_API_KEY}` placeholder; the key lives in the out-of-repo secrets file and
renders into a gitignored `settings.runtime.yml` at bring-up (via a gitignored
compose override). Committed + pushed: `64b202d` (scrub the secrets-file path from
`fmp.rs` doc-comments + gitignore the runtime files; full cargo gate green) and
`8790812` (Serper engine + widened set + `web-research.md §Search backend` note).

## Current state

**The big run is fully prepped but NOT launched** — the user decided to start it
in a **new session**. Done this session: dev portfolio store **wiped to a clean
debut** (continuity 30 reports / 67 vectors / 14 baselines preserved; `job_runs`
history kept → next run = **id 5**; pre-wipe backup in the session scratch), Serper
key added by the user, infra brought up and then **torn down** (Ollama, OrbStack,
SearXNG all stopped). The re-attempt gate stays met; the engine list is
**provisional** pending the run's measured serve rates. Findings 4–6 still ride the
full run (unchanged). Full bring-up mechanics + the next-session sequence live in
the `searxng-orbstack-bringup` memory; the carried unrelated follow-ups are
unchanged from the prior handoff.

## Open questions

- When to launch — the user said "we will start the big run in a new session"; do
  it on their go, don't propose it unprompted.
- The **permanent** SearXNG engine set (and Serper SearXNG-side vs app-side
  long-term) — a post-run call informed by measured serve rates; the committed
  `settings.yml` set is provisional.
- Whether a second full pass runs — the user's call after run 1.
- Findings 4–6 all want the run's telemetry first; no code candidate left.

## Where to start

**This is the launch session.** On the user's go: bring up OrbStack → render
`settings.runtime.yml` (Serper key from the out-of-repo secrets file) +
`docker-compose up -d` → Ollama (`NUM_PARALLEL=1`) → **re-probe engines + confirm
Serper serves** (block set shifts daily) → confirm the store is still a clean debut
→ `cargo run` → confirm debut stamps
(`portfolio-v32` / `checkpoint-v8` / `evidence-floor-v4` / `grade-v2.3`) → trigger →
read `data-health` early. Sequence + mechanics: the `searxng-orbstack-bringup`
memory.
