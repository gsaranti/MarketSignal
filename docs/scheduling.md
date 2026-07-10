# Job Execution

The application runs its work as **on-demand jobs** — the Market Signal Report
plus the two local-suite jobs, Portfolio Analysis and Trade Opportunities —
directly on the user's machine. There is no automatic scheduler; a job runs only
when the user starts one. The user chooses the cadence (for example, generating a
report after a notable market development), and each job frames its picture
relative to its own previous run rather than assuming a fixed interval.

The on-demand jobs are:
- the **Market Signal Report** — the macro / market thesis (detailed below)
- **Portfolio Analysis** — grading the user's holdings
- **Trade Opportunities** — discovering new ideas

## The Market Signal Report Job

The report job:
- analyzes market behavior since the previous report
- evolves the macro thesis
- evaluates geopolitical and economic developments
- assesses sector leadership and weakness
- reads inflation, rates, and liquidity conditions
- tracks AI infrastructure and technology trends
- gauges market positioning and sentiment
- identifies forward-looking risks and opportunities
- notes upcoming market-moving events
- retrospectively evaluates prior assumptions and thesis evolution
- retrospectively evaluates prior report accuracy and thesis quality

The end-to-end steps that run inside this job are defined in
[report-workflow.md](report-workflow.md).

## Job States

A job ends in one of these states:

- **Successful** — the workflow completed and produced its result (a report, or a Portfolio / Trade Opportunities run).
- **Failed** — execution started but could not complete because required
  services, APIs, or model providers were unavailable, or because of API limits,
  token exhaustion, malformed responses, or model execution errors. See
  [Offline Behavior](#offline-behavior) and [Error Handling](#error-handling).
- **Skipped** — a second execution was rejected because another job was already
  running (the single global run slot). See
  [Concurrent Job Protection](#concurrent-job-protection).
- **Cancelled** — the user stopped a running execution from the run tracker
  before it completed. A cancelled run produces no result and, unlike a failed
  run, raises no warning. See [run-tracking.md §Cancellation](run-tracking.md#cancellation).

There is no *Missed* state: because these jobs are user-initiated rather than
scheduled, a job is never "due" while unattended, so there is nothing to miss.

The two local-suite jobs are started from their own controls and gated
separately, but share the lifecycle above (states, the global run slot, offline
behavior, error handling); their end-to-end pipelines are specified in
[portfolio-workflow.md](portfolio-workflow.md) and
[trade-opportunities-workflow.md](trade-opportunities-workflow.md).

## Application Runtime

A job runs only while the user has the application open and has started one;
there is no background processing. The application is an ordinary windowed app —
closing the window quits it, and nothing runs when it is not open. A job in
flight when the user quits is simply ended (it leaves no result, consistent with
the run-is-not-a-report rule in [run-tracking.md](run-tracking.md)).

## Offline Behavior

If, during a run, the machine:
- loses internet connectivity
- cannot reach APIs
- cannot access configured model providers
the job fails cleanly.

The application:
- ends the current job
- stores the failure state
- displays a warning inside the Persistent Warning Area

Network reachability is not checked before a run begins — a job is always
attempted, and an unreachable provider surfaces as a Failed job (with an
immediate error to the user, since a run is always user-initiated) rather than a
pre-run gate. Two local-suite exceptions: the run-gate **Ollama daemon health
check** — the one *blocking* pre-run reachability check, an unreachable daemon
blocking the attempt **inline, before any job exists**, never a Failed job
([local-models.md §Serving runtime](local-models.md#serving-runtime)) — and the
live **SearXNG pre-run probe**, which only informs a consent modal and never
blocks or gates ([interface.md §Pre-run web-research
notice](interface.md#pre-run-web-research-notice-local-suite)). Each job's execution gate checks
credential *presence*, not
connectivity (the report's gate is in [configuration.md](configuration.md); the
local jobs' in [local-models.md](local-models.md) and [schwab-integration.md](schwab-integration.md)).

## Concurrent Job Protection

Only one job may run at a time across the whole application. The Market Signal
Report and the two local-suite jobs (Portfolio Analysis, Trade Opportunities)
share a **single global run slot**, so they are mutually exclusive — and the
lighter Portfolio controls hold the same slot: the engine-only **quick check**
and the view-only **Pull holdings**
([portfolio-analysis.md §Triggering](portfolio-analysis.md#triggering)) —
matching the latest-run-only run tracker ([run-tracking.md](run-tracking.md)).

If a job is currently running and another execution is attempted, the second
execution is skipped.

The application logs the skipped execution.

## Job Status Visibility

The application displays:
- last successful run time
- currently running job state, with live per-step and per-request progress in the
  run tracker (see [run-tracking.md](run-tracking.md))
- last failure state
- last cancelled run
- skipped job events

## Generating a Report

The user starts a report from the application's report-generation control.

Users can also **cancel a running job** at any point from the run tracker.
Cancellation is cooperative and the run is recorded as a Cancelled job — see
[run-tracking.md §Cancellation](run-tracking.md#cancellation).

The execution gate that prevents a report from running until all required agent
models and provider credentials are configured lives in
[configuration.md](configuration.md).

## Error Handling

If a job fails because of:
- API limits
- token exhaustion
- provider failures
- malformed responses
- model execution errors
the application:
1. cleanly ends the job
2. stores the failure state
3. displays a warning inside the Persistent Warning Area

The Persistent Warning Area de-duplicates warnings within each category. See
[interface.md §Persistent Warning Area](interface.md#persistent-warning-area) for
the canonical rule.
