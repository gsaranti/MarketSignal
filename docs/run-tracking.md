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
- **Every data request, one row per request.** During the baseline gather, each
  external API request appears as its own row as it is made, then resolves to a
  pass or a fail. A failed request shows why it failed (for example *unavailable*,
  *rejected*, or *malformed*). A request that is never made — because an earlier
  request to the same provider was rejected, short-circuiting the rest — produces
  no row, so the rows stay one-to-one with the network calls actually made.
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
- **After a run ends**, its trace lingers as a reopenable **run log** — for the
  latest run, until the next run begins or it is dismissed — reached from the
  footer's **View run log**.

Selecting any report from the sidebar while a run is in flight shows that report
and leaves the run running in the background; the footer returns the user to the
tracker. When a run finishes, the application shows the new report if the user is
still watching the tracker, and otherwise leaves them where they are.

See [interface.md](interface.md) for where the tracker sits in the layout and
[scheduling.md](scheduling.md) for job states and controls.
