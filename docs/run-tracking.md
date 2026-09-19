# Run Tracking and Cancellation

While a job is running, the application shows a live **run tracker** in place of the **running job's own page** — a report run replaces the report, a Portfolio run replaces the Portfolio page, and a Trade Opportunities run (Discover or Audit) replaces the Trade Opportunities page — so the user can watch the job progress and stop it at any point.
It is one shared tracker (the local-suite jobs stream into the same component and event seam as a report run — [interface.md](interface.md)), placed on whichever page owns the run.
The tracker is presentation only — it observes the workflow defined in [report-workflow.md](report-workflow.md) (or the local job's workflow doc); it does not change what the job does.

## What the Tracker Shows

When a run begins, the owning page (the report pane for a report run) is replaced by the tracker, which streams the job's progress as it happens:

- **Each step as it initiates.**
  The workflow's stages appear in order as they start and resolve — the credential and configuration check, the baseline market-data gather, the coverage check, the main agent writing the report, and saving the result.
- **Every data request, one row per request.**
  During the baseline gather — and the research phase, when it runs — each external request appears as its own row as it is made, then resolves to a pass or a fail.
  A failed request shows why it failed (for example *unavailable*, *rejected*, or *malformed*).
  A *request* here is one logical fetch — a single data series, or a single research query — not a single network packet: adapter retries ordinarily belong to that one request's row.
  Local web-page fetching is the exception: its application-managed retry gets its own row, with each admitted attempt counted against the holding's budget.
  A request omitted by a provider short-circuit produces no row.
  A local web-fetch request served by failure memory or suppressed by a host cooldown does produce a row, explicitly naming the reason and whether any live attempt was spent.
  So each row corresponds to one request the workflow chose to make, resolved to its final outcome.
  A row belongs to the step that issued it, and that ownership is stated by the run itself — every request event is stamped with its owning step as it is emitted — never inferred from arrival order or request naming.
  A row that arrives owning no step is shown in its own small "requests outside any step" list rather than being forced into a step, so a stray request can never make a step look failed.
  A row reads *ok* only when the request landed usable data: a response that parsed but carried nothing usable shows *empty*, and an unreadable response shows *malformed* with its cause on the row.
  A research row — a local job's web search or page fetch — also names what it asked for: the search query, or the address of the page, shown beside the topic it served.
  The subject is a disclosure button: activate it with Enter, Space, or a pointer to reveal the full text on a wrapping line beneath the row.
  Activate it again to hide that line.
  A completed row carries its short outcome note when the request left one, such as the number of hits a search returned or that a page was served from the document cache.
  A failed local page fetch includes its error source chain in the detail, preserving the available underlying cause through remembered failures and host cooldowns ([web-research.md §Failed fetch memory and bounded retry](web-research.md#failed-fetch-memory-and-bounded-retry)).
- **The main agent's report, streamed live.**
  As the main agent writes the report, its text streams into the tracker as it is produced, rather than appearing only once the report is finished.
- **The main agent's reasoning, streamed live.**
  When the selected model reasons before answering (extended thinking), a summary of that reasoning streams into the tracker as a quieter, subordinate stream shown above the report text — a view into how the thesis is being weighed as it forms.
  Models that do not surface their reasoning simply show none; nothing breaks and no error is raised.
- **Each analyst's reasoning, streamed live.**
  While the three analysts (Bull / Bear / Balanced) run concurrently, each one's reasoning streams into its own labeled pane under the analyst step — the same quieter, subordinate stream as the main agent's. Only the reasoning streams here, not the analyst's review itself; an analyst whose model does not surface reasoning simply shows none.
- **Step-scoped reasoning, streamed live (local jobs).**
  A local job's per-item interpretation stages stream their reasoning onto the step the stage belongs to — a Portfolio run's "Analyze {symbol}" step shows the model's live thinking for that holding while it runs, in the same subordinate reasoning pane the report steps use.
  The structured verdict itself never streams; only the reasoning does, so the schema-validated result remains the single source of truth.

Every reasoning pane is bounded in height and scrolls on its own, so a long run's earlier steps stay compact and the tracker scrolls between steps rather than through every thought.

The tracker is a live view of one run.
Its contents are kept for the current application session and reflect the **latest run only**; they are not persisted across restarts.

## Thought-log capture (diagnostic)

Because the tracker's reasoning panes are transient, a diagnostic sink can capture them to disk.
It decorates the live progress reporter and appends every streamed **thinking** delta to one plain-text file per stream: `main-agent.txt`, `analyst-<posture>.txt`, and `<step-key>.txt` for step-scoped reasoning (each per-holding step — its research turns, interpretation and action calls alike).
The files land under a per-run folder at `<data-dir>/thought-logs/<UTC-timestamp>-<run-id-prefix>/`.
It exists because a failed live run otherwise leaves no reasoning evidence; the 2026-08-10 attempt-1 analysis rested on screenshots of the panes.
The capture is thoughts-only by construction: the main agent's report body persists as the report itself, and a review body or structured verdict never streams, so neither can ever land in a log.

A local-model call additionally writes a **call fence** around its thinking.
Every local chat call emits a `model-call-started` and a `model-call-finished` event on the progress seam, and the sink renders them as a header and a trailer line in the owning step's file (`run.txt` when the call fired with no step open), so a holding's file reads as a call timeline rather than every call's reasoning run together.
The header carries the run's call number, the caller's stage label (the interpretation, role-risk or action stage, a distillation stage, or a research topic's gathering turn or synthesis leg), the model, the `think` flag, whether the call streamed, whether tools or a format grammar rode the request, `num_ctx`, `num_predict`, the prompt's size in characters, and the UTC start time.
The trailer carries the outcome, the elapsed time, and the daemon's prompt and generated token counts and stop reason when reported, or a failed call's top-level message capped to about 200 characters.
Fences carry labels and counts only, never prompt or body text.
Every local call is fenced.
A non-thinking distillation call therefore leaves adjacent fences with nothing between.
The tracker does not render the two boundary events.
They exist for this sink and the stderr tee.
A streamed call's thinking lands between its fences as it streams.
A non-streaming call — the research loop's gathering turns and synthesis call — forwards its thinking whole once the reply lands.
A non-streaming call that meets a handled timeout or daemon error leaves a header and a failed trailer with no thinking between them.
Abrupt app termination while a non-streaming call is still in flight leaves only its header, which names the interrupted call.
The cloud report job's calls carry no fences.
Its `main-agent.txt` and `analyst-<posture>.txt` streams are unchanged.

The sink is permanent code with build-gated behavior: debug builds capture by default (opt out with the variable below), and release builds stay silent unless explicitly opted in.
`MARKET_SIGNAL_THOUGHT_LOG` (`1`/`true` or `0`/`false`) forces capture on or off in either build.
The folder rides the same data-directory resolution as every store (`MARKET_SIGNAL_DATA_DIR` override; `dev/` nesting in debug builds).
The newest ten run folders are kept: pruning happens only after a run's first capture — a call fence or a thinking delta — has landed on disk, so a run that captures nothing (a quick check, a blocked attempt) — or whose capture fails outright — never spends an old log without a replacement existing.
Pruning is shape-guarded: only folders matching the sink's own exact timestamp-and-id naming are counted or deleted, so anything else in the directory is not its to remove.
The logs are loose diagnostic files — outside SQLite, outside the portability archive, and outside every store retention rule.
Capture is best-effort: a run that streams no thinking and issues no local-model call creates no folder, and an I/O failure disables capture for that run with one log line, never the run itself.
Appends are synchronous and unbuffered — the crash-honesty the sink exists for, since everything streamed before a failure is already on disk; the accepted cost is that a stalled disk would stall the run, tolerable for a debug-gated diagnostic.

## Cancellation

The user may cancel a running job at any point from the tracker.

Cancellation is cooperative: the application stops the run at the next safe checkpoint — between steps, between data requests, and while an agent is streaming (the main agent or any of the three analysts) — rather than interrupting a request already in flight.
In practice the run stops within a request or two of the cancel.

A cancelled run:
- does not produce a report,
- is recorded as a **Cancelled** job, distinct from Failed and Skipped (see [scheduling.md §Job States](scheduling.md#job-states)),
- does **not** raise a failed-job warning, because it was intentional.

A **Portfolio Analysis** run's failed or cancelled terminal state additionally offers **Resume** while the interrupted run's checkpoints are eligible — with a line stating what resume keeps, or why it is unavailable; the contract is canonical at [portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture).
A contained internal error — a panic inside the analysis — reaches that same failed terminal state ([portfolio-analysis.md §Failure posture](portfolio-analysis.md#failure-posture)).

## A Run Is Not a Report

A report appears in the Recent Reports sidebar only once it has been generated and saved successfully.
An in-progress run is never shown as a report; it lives only in the tracker.
A run that is cancelled or fails therefore leaves no report behind and removes nothing from the report list.

## Reaching the Tracker

The run tracker is reached from the job status footer, which is the home for the running job:

- **While a run is in flight**, the footer offers **View progress**, which opens the live tracker.
- **After a run ends**, its trace lingers as a reopenable **run log** for the latest run, reached from the footer's **Latest run log**, and remains available for the rest of the session.
  Returning to the report from the log (**Back to report**) leaves it reopenable; it is replaced only when the next run begins (latest-run-only), and cleared only when the application quits.

Selecting any report from the sidebar while a run is in flight shows that report and leaves the run running in the background; the footer returns the user to the tracker.
When a run finishes, the application shows the new report if the user is still watching the tracker, and otherwise leaves them where they are.

See [interface.md](interface.md) for where the tracker sits in the layout and [scheduling.md](scheduling.md) for job states and controls.
