# Run Tracking and Cancellation

While a Weekly Market Report job is running, the application shows a live **run
tracker** in place of the report, so the user can watch the job progress and stop
it at any point. The tracker is presentation only — it observes the workflow
defined in [weekly-report-workflow.md](weekly-report-workflow.md); it does not
change what the job does.

## What the Tracker Shows

When a run begins, the report pane is replaced by the tracker, which streams the
job's progress as it happens:

- **Each step as it initiates.** The workflow's stages appear in order as they
  start and resolve — the credential and configuration check, the baseline
  market-data gather, the coverage check, the main agent writing the report, and
  saving the result.
- **Every data request, one row per request.** During the baseline gather — and
  the research phase, when it runs — each external request appears as its own row
  as it is made, then resolves to a pass or a fail. A failed request shows why it
  failed (for example *unavailable*, *rejected*, or *malformed*). A *request* here
  is one logical fetch — a single data series, or a single research query — not a
  single network packet: when a request meets a transient failure (a rate limit or
  a brief server error) the application retries it a bounded number of times
  automatically, and those retries belong to that one request's row rather than
  spawning new ones. A request that is never made — because an earlier request to
  the same provider was rejected, short-circuiting the rest — produces no row. So
  each row corresponds to one request the workflow chose to make, resolved to its
  final outcome.
- **The main agent's report, streamed live.** As the main agent writes the weekly
  report, its text streams into the tracker as it is produced, rather than
  appearing only once the report is finished.

The tracker is a live view of one run. Its contents are kept for the current
application session and reflect the **latest run only**; they are not persisted
across restarts.

## Cancellation

The user may cancel a running job at any point from the tracker.

Cancellation is cooperative: the application stops the run at the next safe
checkpoint — between steps, between data requests, and while the main agent is
streaming — rather than interrupting a request already in flight. In practice the
run stops within a request or two of the cancel.

A cancelled run:
- does not produce a report,
- is recorded as a **Cancelled** job, distinct from Failed and Skipped (see
  [scheduling.md §Job States](scheduling.md#job-states)),
- does **not** raise a failed-job warning, because it was intentional.

## A Run Is Not a Report

A report appears in the Recent Reports sidebar only once it has been generated and
saved successfully. An in-progress run is never shown as a report; it lives only
in the tracker. A run that is cancelled or fails therefore leaves no report behind
and removes nothing from the report list.

## Reaching the Tracker

The run tracker is reached from the job status footer, which is the home for the
running job:

- **While a run is in flight**, the footer offers **View progress**, which opens
  the live tracker.
- **After a run ends**, its trace lingers as a reopenable **run log** for the
  latest run, reached from the footer's **Latest run log**, and remains available
  for the rest of the session. Returning to the report from the log (**Back to
  report**) leaves it reopenable; it is replaced only when the next run begins
  (latest-run-only), and cleared only when the application quits.

Selecting any report from the sidebar while a run is in flight shows that report
and leaves the run running in the background; the footer returns the user to the
tracker. When a run finishes, the application shows the new report if the user is
still watching the tracker, and otherwise leaves them where they are.

See [interface.md](interface.md) for where the tracker sits in the layout and
[scheduling.md](scheduling.md) for job states and controls.
