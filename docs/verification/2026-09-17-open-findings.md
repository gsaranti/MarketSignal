# Open findings — the Portfolio local-model calls (2026-09-17)

This is the single work list for the Portfolio local-model surface from 2026-09-17.
It supersedes the attempt-6 fix list and the per-slice records as the list of open findings; those files are history and are not consulted for what is open.
Every entry the prompt-rewrite series closed (`portfolio-v40` through `portfolio-v44`) is omitted.
Entry 12, the rendered ledger (`portfolio-v45`), is landed and kept for its rulings and the checks attempt 7 reads; entries 1 and 2 stay as the history it superseded.
What remains is app-side work the prompts never touched, the checks attempt 7 reads for the landed prompts, and the watches for that attempt.
Each entry is a decision, the evidence it rests on, the acceptance check and the stamp it moves; its ruling status is on the entry.
A finding from attempt 7 is appended here as the next number, never in another file.

## Standing rules

- Pre-release, no data compatibility work: a persisted-shape change moves its stamp and the next attempt re-wipes.
- The debut stamps are `portfolio-v46` / `checkpoint-v12` / `evidence-floor-v5` / `grade-v2.3` / `targets-v6` / `pre-profit-v4` / `quick-check-v4`, portability format 8.
  Any further prompt or schema change lands as `portfolio-v47`; the checkpoint and evidence-floor stamps move only where an entry says its persisted shape changes, confirmed at plan time.
- Nothing is adopted on one holding's impression; each entry names the check that admits it.
- Checks are behavioral where the finding was: a source-evidence input and an expected persisted outcome.
  Re-think marker counts stay a diagnostic, never the gate.
- Every slice keeps the existing guards under test: the gathering-tools / synthesis-grammar separation, blank-findings rejection, and citation admission.
- A field whose meaning changes updates its producer, its consumers and its single-home doc section in the same slice.
- Every entry is ruled through the selector before implementation starts; plan flags are ruled with the code in view, not before.
- The fixed-evidence harness is deprecated (ruled 2026-09-17).
  The next test after the remaining fixes is attempt 7, a book-scale run from a re-wiped store; every landed prompt and every fix that lands before it is admitted or faulted on attempt 7's evidence, its first holdings read early.
  The v40 / v41 / v42 fixed-set live read is not run; its checks read on attempt 7 instead (§Verification on attempt 7).
  The harness is rebuilt after attempt 7 if the run shows a need; an entry that needs repeats on byte-identical packets waits on that rebuild.
  Ruled 2026-09-18: the harness code stays in the tree whole.
  The thirteen offline tests run in `cargo test` — the attempt-6 6g outcome regression is the rendered-ledger slice's never-dropped witness, its expectations re-derived under the surviving classes, and the two-part / no-app-concept pins hold every prompt surface — and the two ignored tests cost nothing in the gate and seed the rebuild.
- Attempt 7 follows the remaining fixes, and the user names the launch session.
  It re-wipes the dev store first, dropping `web_source_state` and clearing `portfolio_runs` and the `portfolio` namespace of `vector_memory`.

## Queue

Before attempt 7, in this order unless ruled otherwise: entry 8; then entries 4 through 7, each its own plan; entry 11's measures set before the launch.
Entries 1, 2 and 12 are closed (12 landed 2026-09-18, superseding 1 and 2).
Entry 3 is re-scoped on attempt 7's evidence and planned after it.
Entries 9 and 10 follow attempt 7 and need a harness, rebuilt if the run shows the need.

## Entries

### 1. A window preposition masks a statement's direction

- Decision: the 6g comparator check must not read "over" in a window phrase ("over a rolling 6-month window", "over two quarters") as an above-word.
  Today the direction reads as mixed and the comparator check is skipped.
- Rests on: the v39 fixed-set read admitted one wrong core — SPMO's "Trailing price return inflects below -10% over rolling 6-month window" on a core of `above -0.1` — the first wrong core admitted since the v37 re-read.
- Check, offline: that sentence on that core downgrades as `comparator-mismatch` instead of keeping.
- Stamp: to confirm at plan time; a validator-only change moved no stamp of its own in the v37 slice.
- Ruled 2026-09-17: a separate small validator slice, not a prompt change; one slice with entry 2.
- Superseded 2026-09-18 by entry 12: the statement is rendered from the core, so no direction word is read.

### 2. "over N units" is not read as a duration

- Decision: the duration parser accepts "for N units" and "N consecutive units" only, so "Net margin sustains below 4% over two quarters" keeps a core that confirms on the first breaching print.
  "over", "across" and "through" N units read as durations.
- Rests on: the v39 read (PSX).
- Check, offline: the PSX sentence downgrades as `qualifier`; "across two quarters" and "through two quarters" likewise.
  Ruled 2026-09-18: the checker's existing short-circuit order stands (series, malformed, unit, comparator, metric, basis, level, no-level, margin, qualifier) and the first failing check names the reason, so a wrong direction outranks a duration.
  "over a rolling N-month window" describes the series and is never read as a duration.
- Stamp: as entry 1.
- Ruled 2026-09-17: the same validator slice as entry 1.
- Superseded 2026-09-18 by entry 12: no duration is parsed; a duration the fields cannot express makes the condition qualitative by the model's own choice.

### 3. Claim dating — the persisted shape and the reconciliation rule

- Decision: carry publication date, fact period and retrieval time as distinct fields through claims and distillation.
  Reconciliation orders on fact period and publication, never retrieval time, and the rule says what wins when two sources cover the same period, when a revision supersedes, and when a period or date is unknown or incomparable.
  The rule itself is the plan's to write.
- Narrowed 2026-09-17: the model-facing half is done.
  Since `portfolio-v43` and `portfolio-v44` the research and distillation messages show each page's publication date and no retrieval timestamp, and the shared holding header carries the run date.
  What remains is the persisted half: `DistilledClaim` carries a single `vintage` field, so the three dates are not distinguishable in the persisted record and no app-side ordering can prefer publication over retrieval.
- Rests on: attempt 6 — ARKF's "announced September 16, 2026, trading March 31, 2025" contradiction and TSLA's "July CY25", both dated by retrieval.
- Check: neither can persist; a claim's persisted record names its publication date and its fact period apart from when it was retrieved.
- Stamp, ruled 2026-09-18: the shape half (new fields on the persisted claim) moves `checkpoint` (to v13, past the landed v12), plus portability's format if the archive carries the topic layer, which the plan confirms; the basis half (the freshness window reading publication instead of retrieval) moves `evidence-floor`.
  The entry as scoped does both.
- Ruling: open.
  Re-scoped on attempt 7's dating checks (§Verification on attempt 7) before planning: if no distilled finding dates a fact by retrieval, the entry narrows to the persisted shape alone.

### 4. Failed-URL memory with failure classes and host backoff

- Decision: a run-scoped record of failed URLs with failure classes, consulted by the fetch tool.
  An exact-URL repeat returns the earlier failure without spending an attempt; a host that denies (401 / 403) gets a short-lived backoff; a transient failure never becomes a permanent ban.
- Rests on: attempt 6's fetch economics — nhtsa.gov 3 attempts on 1 URL, wsj 4 on 2, reuters 6 on 4; phillips66 11 attempts on 9 distinct URLs and progyny 15 on 13, where URL reuse alone barely helps.
- Check, as three separate expectations on the attempt-6 log offline and on attempt 7's rows live: exact-URL reuse removes the repeats; host backoff bounds a denying host to a set number of live attempts per window; a transient failure is retried once and never banned.
- Stamp: none.
- Ruled 2026-09-18: run-scoped memory keyed by URL and by host, spanning every holding of one run and dropped at run end; `web_source_state` keeps its telemetry role untouched.
- Ruled through the selector 2026-09-18: the first 401/403 starts a five-minute cooldown for the denying host and URL; skips never extend it, and expiry admits a probe.
  Exhausted transient failures and unclassified failures have a 30-second URL cooldown; only positively classified transient failures retry, once after one second, with `Retry-After` a minimum and budget/cancellation checked before issue.
  Retryable HTTP statuses are 408/429/500/502/503/504; typed timeouts, connection resets, connection aborts and broken pipes qualify, while opaque DNS/TLS errors do not.
  Exact parsed URL keys drop fragments but preserve paths, trailing slashes and queries; exact lowercase host keys keep `www` and subdomains separate.
  Each invocation, including resume, starts empty; redirect denial cools the denying host without changing requested-host telemetry.
- Offline acceptance uses archived attempt-6 URL/status fixtures plus synthetic time: nhtsa's repeated URL, phillips66's distinct denied URLs and progyny's unknown-cause failures remain separate cases.
  The archived progress rows have no per-request timestamps and some omit causes, so historical cooldown savings and transient classifications are not inferred from them.
  Fake-clock and mock-HTTP tests cover expiry, retry recovery/exhaustion, cancellation, budget limits, `Retry-After`, redirect guards, telemetry and invocation reset.
- Implementation: entry 4's code and contract are implemented and review is complete; attempt-7 live-effectiveness acceptance remains pending.
  The canonical policy is `docs/web-research.md` §Failed fetch memory and bounded retry.
  No prompt, persisted-shape or version-stamp change.
- Review follow-up (2026-09-18): a newly discovered redirect alias now remembers a suppressed destination's failure with its original expiry.
  The adapter/runner regression failed before the fix and passes after it: repeating the alias, including in another holding, adds no request, attempt charge or telemetry sample, and the alias becomes eligible at the original host expiry.
- Verification (2026-09-18, after the review follow-up): `cargo test` passed (1,505 library tests and 32 integration tests; 33 ignored), including fourteen entry-4 regressions; `cargo clippy --all-targets --all-features` passed without warnings, `npm run build` passed, and `git diff --check` passed.
  Mock-HTTP tests required execution outside the sandbox to bind local ports; no live-source or model run was performed.

### 5. Evidence reuse across a holding's topics and a visible remaining budget

- Decision: reuse successfully fetched raw evidence across a holding's topics before searching again; expose remaining replies and keep the topic questions visible so gathering judges what is still unanswered.
- Ruled through the selector 2026-09-18: show bounded page text before searching; the model judges unanswered questions from the shown pages with no question-status reporting; show replies left in this pass, including the current reply, refreshed before each gathering request.
  Existing limits remain unchanged.
- Implementation: a transient holding-scoped inventory supplies ordinary topics and follow-up passes in first-retrieval order within the existing initial-message allowance, preserving original source dates, annotation and redirect lineage.
  Explicitly requested pages precede reused pages in the synthesis admission path; current URL policy, usable-body admission and shown-source citation validation still bind.
  Other topics' findings and transcripts stay out, and the disconfirming pass retains its dedicated contrary-search behavior without automatic page injection.
  The canonical contract is `docs/web-research.md` §The research loop and context management.
- Check, offline: a two-topic and follow-up script reuses source bodies without another search or fetch, with original dates and redirected seed lineage; the countdown reads 8 through 1 with unchanged retries; bounds, policy refusals, explicit source replacement and omitted-source citation rejection remain under test.
- Check, on attempt 7: the cap-hit share falls beside attempt 6's without raising the cap.
  Compare the same completed holdings' topic-pass population used for attempt 6's recorded 24/33 baseline, report the denominator and skipped or failed passes separately, and read evidence coverage alongside the rate so empty passes or a different earlier bound cannot count as success.
  The full-book rate is reported separately; this is an observational comparison with the other pre-run fixes also present, not an isolated attribution to entry 5.
- Stamp: `portfolio-v46`; no persisted-shape change, so `checkpoint-v12` and portability format 8 remain unchanged.
- Verification (2026-09-18): `cargo test` passed (1,510 library tests and 32 integration tests; 33 ignored), including five entry-5 regressions; `cargo clippy --all-targets --all-features` passed without warnings, `npm run build` passed, and `git diff --check` passed.
  The full mock-HTTP suite required execution outside the sandbox to bind local ports; no live-source or model run was performed.
- Independent review is complete: Metis approved, and Claude's separate review agreed and reported passing full gates.
  Attempt-7 live-effectiveness acceptance remains pending until the user-authorized run.

### 6. The fetcher declares the SEC User-Agent and names its failure cause

- Decision: declare the SEC fair-access User-Agent on web fetches (the EDGAR API legs already do); render the error source chain into the progress row's detail, not only the outer "fetching <url>" context.
- Rests on: attempt 6 — sec.gov pages denied through the web tool, and progyny-class failures with no cause in the tracker row.
- Check: the web fetcher's request to sec.gov carries the declared User-Agent and a 403 still classifies as denied (live availability is not the test); a progyny-class failure names its cause in the tracker row.
- Stamp: none.
- Ruling: approved 2026-09-18 — reuse the existing SEC identity for `sec.gov` and its subdomains, selected again on each redirect hop; other hosts retain the browser identity.
  Preserve entry 4's existing source-chain propagation and add regression coverage rather than rewriting it.
  Include the narrow tracker-documentation correction for separate web-fetch retry rows.
- Implementation: complete 2026-09-18; Metis review approved after independently rerunning all verification gates.
  Claude's separate review agreed and reported matching green gates; its stale review-status nit is corrected here.
  Six offline regressions cover SEC host boundaries, outgoing headers through redirects into and out of SEC, denied classification and telemetry, cooldown and remembered-failure accounting, and nested causes reaching progress details from synthetic errors and malformed HTTP responses/bodies.
  The malformed-response tests exercise the production fetcher, failure memory and research runner; the historical Progyny cause remains unknown.
  Verification passed: 1,516 library tests and 32 integration tests (33 ignored), warning-free `cargo clippy --all-targets --all-features`, `npm run build`, `npm test` (46 pure-module and 266 component tests), and `git diff --check`.
  The HTTP fixtures required local-port access outside the sandbox after its initial `Operation not permitted` failure.
  No prompt, schema or persisted shape changed; no version stamp moved.
  Live effectiveness remains deferred to attempt 7; no live source/model probe, harness run, daemon restart or dev-store wipe was performed.

### 7. Per-call telemetry with explicit semantics

- Decision: record per call, with explicit semantics, the original packet size, the whole-call elapsed, the thinking characters and the returned API counters.
  Stop describing the fence's `generated` count as thinking-plus-content; on this runtime's two-phase path it is the second task's count.
- Rests on: attempt 6's token telemetry, where the fence counts undercount because the runtime runs thinking and content as two tasks.
- Check: a persisted prompt-usage row carries the four measures with their names, and the fence trailer's count is labelled as the runtime returns it.
- Stamp: the prompt-usage row is persisted; the plan confirms whether the checkpoint stamp moves.
- Ruling: open.

### 8. Effective sampling parameters captured from the serve log

- Decision: capture the existing runtime sampler blocks for thinking-general and non-thinking-general, including inherited defaults, following [the operational capture procedure](../local-model-operations.md#capturing-effective-sampling-parameters).
  The archived attempt-6 log already prints these blocks at task launch; a model-load snapshot alone misses subsequent per-call profile changes.
- Check: attempt 7's configuration section quotes fresh, complete `sampler chain` and `sampler params` blocks for both profiles from its own serve log, identifying the runtime version, model, profile and task with source log locations and timestamped context alongside the app-requested settings.
  Missing evidence remains explicitly unverified; attempt-6 values such as `repeat_penalty = 1.100` are historical evidence, not substitutes for fresh capture.
- Stamp: none.
- Ruled 2026-09-18: documentation only, using the existing sampler blocks for both profiles; no code or stamp changes.
  The capture procedure is documented; the attempt-7 evidence check remains pending until the user-authorized run.

### 9. Sampling-profile comparison on the action call

- Decision: the current "Thinking — general" row against the vendor "Thinking — precise" row (`docs/local-model-operations.md §Sampling settings`), the packet byte-identical, thinking on, the same repeats per holding each way with thinking captured; the model, runtime and `num_ctx` held fixed; greedy decoding excluded (vendor-warned).
- Rests on: rung flips on byte-identical packets — SPMO and ARKF between sell-all and trim across repeats on the v36, v37 and v38 reads.
  A debut-only exposure, since a continuity run renders the prior model-chosen action as its baseline; sampling is a plausible cause no observation yet isolates.
- Requires entry 8 first, so each run's effective options are on the record and the comparison is attributable.
  Requires repeats on byte-identical packets, so it needs a harness.
- Check, judged in this order: no core refused on re-authoring (`holds-at-authoring`, `margin-implausible`; the sentence is the app's render since entry 12, so there is no wrong core to admit) and every rung inside its holding's repeat set under both profiles; then the between-repeat rung agreement per holding; then elapsed time; re-think markers diagnostic only.
- The precise row is adopted for the action call only if correctness holds and rung agreement rises.
  An adoption updates the ops doc's stage mapping and is a per-call option, never a load-time change.
  The interpretation call is a second leg only if the first shows an effect.
- Ruled 2026-09-17: adopt as an entry; follows attempt 7 on the rebuilt harness, and attempt 7's own rung flips, if any, decide whether the rebuild is needed for it.
  Its place relative to entry 10's action leg is a plan flag; the recommendation is before it, so entry 10 compares against the better thinking-on profile.

### 10. Non-thinking experiments

- Decision: non-thinking synthesis on frozen evidence, comparing claim retention, dates, citation resolution and partial-coverage accuracy as well as time, with repeats for variability; then non-thinking action separately on the investment-only packet; interpretation stays thinking-on throughout.
- Requires a harness with a synthesis leg over frozen pages, which no harness has had.
- Ruled 2026-09-15: run them in that order after the packet fixes; synthesis is the expected win, and action may be dropped if judgment quality suffers.
  Ruled 2026-09-17: follows attempt 7, on the rebuilt harness if the run's stage times show the need.

### 11. Success measures

- Decision: end-to-end acceptance and useful evidence retained per unit of elapsed time, per-stage median and worst-case latency, retries, source drops and thought length; re-think marker counts stay a diagnostic, never the gate; the user's time budget is set explicitly.
- Needs entry 7 so the measures are honest.
- Ruling: open; the measures are set before attempt 7 launches so the run is judged by them.

### 12. The ledger statement is rendered from the core

- Decision: a quantitative condition is authored as its core plus a short name; the app renders the statement the user reads and the engine runs from the core, and the prose-versus-core parser is deleted.
  A new or superseding core that already holds on the authoring surface is refused as `holds-at-authoring`; a carried-verbatim core is exempt.
- Rests on: four Codex rounds on entries 1 and 2 without convergence — reverse-parsing free English to reconcile two representations of one fact has no closed form — and the attempt-6 record, where the model's sentence and its core disagreed on about a quarter of its conditions.
- Check, offline: the render pins one row per unit family; the stub's debut trigger at a spot past its level and PGNY's 300% growth floor refuse as `holds-at-authoring` on the authoring surface; the attempt-6 fixture outcomes are re-derived (`kept`, `qualitative`, `series-uncomputable`, `margin-implausible`, `holds-at-authoring`) and never dropped; a split re-basis re-renders the price sentence from the re-based core.
  Check, on attempt 7: every kept core's statement is its render, the labels read as names without figures, and the `holds-at-authoring` refusals are read per holding with their shown values.
- Stamp: `portfolio-v45` (prompt and contract), `checkpoint-v12` (`label` on the persisted condition, the statement's meaning), portability format 8.
- Ruled 2026-09-18, with the code in view: the label is a separate persisted field; the guard tests new and superseding cores only; the three stamps move; the render carries name, metric, basis, direction, level, confirmation and margin; the continuity prompt shows a kept core raw beside its name; the fixture regression stays with re-derived expectations; a refused condition keeps its render from the draft core; no frontend change (a conditions list is a separate display slice).
- Review rulings 2026-09-18: an off-scale value (negative equity, a non-positive P/E) skips the guard like a missing one, mirroring the evaluator's admissibility gate; a refused condition's continuity row carries one data phrase per class; the synthetic role/risk fixture passes fund metrics so the guard runs on the expense ratio offline; every reviewer nit taken.
- Codex 2026-09-18, three P2 fixed: the import guard refuses format v7 with the other pre-release shapes (no compat rung, as the format note already claimed); the quick check re-renders a re-based price condition so the attention flag names the level it compared; the continuity prompt scopes the not-yet-crossed rule to a new condition and keeps a carried one unchanged even after a crossing.
- Landed 2026-09-18.

## Verification on attempt 7

Every check below reads on attempt 7, its first holdings read early, from the persisted rows, the tracker and the thought logs.
The attempt-6 holdings in book order (TSLA, PSX, SPMO, ARKF, DIA, PGNY) are the comparison set; where a check names a fixed-set count, that count is the reference for the same holding, not a bar.
A check that fails becomes an entry here, numbered on from 13.

Interpretation (`portfolio-v40`):

- interpretation markers per call are counted beside attempt 6's per-holding values and the v39 fixed-set rate (10.2 per call), and the causes that remain are read;
- the refusals — `margin-implausible`, `holds-at-authoring` — are read per holding beside the v39 level-class counts (eight, five and three on eighteen calls), every kept core's statement is its render, and the labels read as names without figures;
- the kept cores' margin-to-level ratios do not sit at the unshown caps (v39 non-price max 40%);
- every target rationale names one of the model's own figures, and no prose field carries account economics;
- no rationale computes with a sentence the interpretation packet no longer carries.

Action (`portfolio-v41`):

- no rationale cites a hurdle state word or computes the hurdle test from a prefix; the rationales that name the hurdle quote the tested returns;
- no rationale argues from what the computed read permits (the permission scan);
- each attempt-6 holding's rung is read beside attempt 6's rung and the fixed-set repeat set (TSLA trim / sell-all, PSX trim, SPMO sell-all, ARKF trim / sell-all, DIA hold, PGNY hold), and a rung outside its engine set is a failure;
- action fence markers are counted beside the v39 rate (forty-two on forty-seven calls), and the ENGINE SET and capital-efficiency shares of the re-think are read;
- the rationales are consistent with the thesis and scenarios now in the packet, and none computes with a sentence the packet no longer carries.

Role/risk (`portfolio-v42`), on the first `role_risk_only` holding the book produces, unread if none does:

- no role read, thesis, scenario row or rationale carries a banned word or an account-economics phrase;
- the ledger's kept cores stay on the fund series with trim and sell families, and the margins do not sit at the unshown caps;
- the role/risk action rationale reads from the packet's sections, names no hurdle and argues from no permission;
- the role read describes the mandate, the exposure, the cost and the risk without a portfolio.

Research (`portfolio-v43`):

- no gathering or synthesis trace reasons about a seed's attribution, the topic-answered boolean or a house-view fit;
- no trace calls a 2026 source future-dated or simulated, and no query asks for 2024 or 2025 material as the latest period;
- no synthesis call runs on a pass with no page body, and the app-assembled pass carries the searching sentence;
- the source id cited on every claim resolves to a shown page;
- the synthesis markers per pass are counted beside the attempt-6 table;
- the exposure-profile topic's findings describe the fund's exposure and name no house view;
- no findings text carries a banned word or a routing word;
- a figure that cannot be right is excluded or persisted as a source defect, never as a claim (PSX's impossible insider-sale amount is the attempt-6 case).

Distillation (`portfolio-v44`):

- no distilled finding or topic summary dates a fact by its retrieval, and no persisted `as_of` is the run date;
- every typed field returned cites a page under the message's SOURCE TEXT, and none is returned on a fund;
- no leading indicator names a driver id outside the rendered list, and none is returned on a first analysis;
- the forward assumption, where returned, is an EPS or revenue figure from guidance, a contract or a filing;
- `unreconciled_topics` stays empty and every topic key returned is one shown;
- the two typed-field lines under RESEARCH SUMMARY carry no banned word;
- the distillation calls' prompt sizes are read beside attempt 6's (15,595–78,045 characters).

Shared, on every call: the empty-string placeholders and the "one JSON object" line hold on the local model as they do on the interpretation call; no live rationale borrows "engine" now that no prompt uses the word; the daemon's grammar honours the per-call enums (a topic-key enum of up to eight keys and the nullable condition-id enum) with no grammar failure.

## Watches for attempt 7

Kept, not scheduled; read at scale rather than fixed here.

- The research leading indicator's direction field and its invented driver ids (SPMO, DIA) — the unverified-driver gap fires as ruled; read the direction accuracy at scale.
- `audit.narrative = null` on every holding that ran the narrative-sentiment topic — establish whether null is the no-hype resting state or a missing input.
- Source-registry coverage (CNBC landing as tier 4 unregistered).
- Distillation original-source allocation and the 12,000-character page cap binding on every stock (a fund's distillation carries no source section since `portfolio-v44`).
- Schwab stock rows arriving without an issuer description, leaving the listing-resolution guard unverifiable — an account-data property to raise with the ingestion leg, not a model-call fix.
- Whether interpretation misses anything on a priced fund now that its distillation carries no typed field.
- Retired 2026-09-18: the v40 read's `no-level` yield on PGNY-type holdings — the class went with the parser (entry 12); read instead whether such holdings' cores refuse as `holds-at-authoring`, a better example being the answer if they do.
- Rung flips between adjacent rungs on the same holding across attempts 6 and 7, the evidence entry 9 waits on.
- Stop rule 1 — the unit and overlay exclusion rate, unread on attempt 6's six holdings.
- The per-domain denied share and the SearXNG engine set at book scale.
- The summary text's ".." artifact.
- Whether ARKF prices again (it moved from role/risk-only to priced under the 2026-09-15 eligibility rulings).

## Not adopted

Ratified 2026-09-15 unless dated otherwise; none is re-raised in a plan.

- A universal "decimal threshold above 1 is invalid" guard: legitimate growth exceeds 100%.
- Rendering the house view into the fund exposure research topic: the fit judgment moved to interpretation instead.
- Compatibility reviews for persisted changes: pre-release, the stamp moves and the store re-wipes.
- Raising `num_ctx`, the output ceiling or the retry counts: no context exhaustion observed; the bounded retries recovered every transport or content failure.
- Sampling changes as a remedy: one variable at a time, and only through entry 9.
- Streaming the research turns: visibility only, no effect on correctness or deliberation.
- An app-assigned noise margin: the margin stays model-authored; the 6g margin guard covers the implausible case.
- A separate compact ledger-authoring call: deferred by construction; interpretation ran 74–181 s per call with no failures.
- "Keep the action firm run to run" on continuity calls only: the debut check is accepted cost.
- The Facts form of the engine-set line: it steered harder on both admission outcomes; the set is one data line with no permission sentence.
- A fixed-evidence read before attempt 7 (ruled 2026-09-17): the harness is deprecated; the rewritten prompts are read on the run.
- A prose check on a ledger statement, in any form (2026-09-18): the statement is the app's render of the core (entry 12; BUILD §Standing constraints), so a field the model must convey goes in the schema, never in the sentence.
