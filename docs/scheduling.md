# Job Execution

The application generates Market Signal reports on demand, running the workflow
directly on the user's machine. There is no automatic scheduler — a report is
produced only when the user starts one. The user chooses the cadence: a report
can be generated as often or as rarely as wanted (for example after a notable
market development), and the analysis always frames the picture relative to the
previous report rather than assuming a fixed interval.

Jobs are responsible for:
- generating Market Signal reports

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

A report job ends in one of these states:

- **Successful** — the workflow completed and produced a report.
- **Failed** — execution started but could not complete because required
  services, APIs, or model providers were unavailable, or because of API limits,
  token exhaustion, malformed responses, or model execution errors. See
  [Offline Behavior](#offline-behavior) and [Error Handling](#error-handling).
- **Skipped** — a second execution was rejected because another report-generation
  workflow was already running. See
  [Concurrent Job Protection](#concurrent-job-protection).
- **Cancelled** — the user stopped a running execution from the run tracker
  before it completed. A cancelled run produces no report and, unlike a failed
  run, raises no warning. See [run-tracking.md §Cancellation](run-tracking.md#cancellation).

There is no *Missed* state: because reports are user-initiated rather than
scheduled, a report is never "due" while unattended, so there is nothing to miss.

## Application Runtime

Report generation runs only while the user has the application open and has
started a job; there is no background processing. The application is an ordinary
windowed app — closing the window quits it, and nothing runs when it is not open.
A job that is in flight when the user quits is simply ended (it leaves no report,
consistent with the run-is-not-a-report rule in
[run-tracking.md](run-tracking.md)).

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

Network reachability is not checked before a run begins — a report is always
attempted, and an unreachable provider surfaces as a Failed job (with an
immediate error to the user, since a run is always user-initiated) rather than a
pre-run gate. The execution gate checks credential *presence*, not connectivity
(see [configuration.md](configuration.md)).

## Concurrent Job Protection

Only one report-generation workflow may run at a time.

If a report job is currently running and another execution is attempted, the
second execution is skipped.

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
