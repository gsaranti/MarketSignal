# Scheduling and Job Execution

The application uses local scheduled jobs that run directly on the user's machine.

Jobs are responsible for:
- generating Weekly Market reports

## Weekly Market Report Job

Runs:
```text
Sunday
9:00 AM local time
```

Focus:
- previous week's market behavior
- evolving macro thesis
- geopolitical and economic developments
- sector leadership and weakness
- inflation, rates, and liquidity conditions
- AI infrastructure and technology trends
- market positioning and sentiment
- forward-looking risks and opportunities
- upcoming market-moving events
- retrospective evaluation of prior assumptions and thesis evolution
- retrospective evaluation of prior report accuracy and thesis quality

The end-to-end steps that run inside this job are defined in [weekly-report-workflow.md](weekly-report-workflow.md).

## Job States

A scheduled or manual job ends in one of these states:

- **Successful** — the workflow completed and produced a report.
- **Failed** — execution started but could not complete because required services, APIs, or model providers were unavailable, or because of API limits, token exhaustion, malformed responses, or model execution errors. See [Offline Behavior](#offline-behavior) and [Error Handling](#error-handling).
- **Missed** — the scheduled execution never started because the application was not running, the machine was asleep, or the application could not start the scheduled execution during the scheduled window. See [System Sleep Behavior](#system-sleep-behavior) and [Missed Job Detection](#missed-job-detection).
- **Skipped** — a second execution was rejected because another report-generation workflow was already running. See [Concurrent Job Protection](#concurrent-job-protection).
- **Cancelled** — the user stopped a running execution from the run tracker before it completed. A cancelled run produces no report and, unlike a failed run, raises no warning. See [run-tracking.md §Cancellation](run-tracking.md#cancellation).

## Application Runtime Requirements

**Application Must Be Running**

Scheduled jobs only run while the application is running.

If the user fully quits the application:
- scheduled jobs do not run
- report generation stops
- no background processing occurs

Closing the application window does not quit the application if the app remains active in the system tray.

## System Sleep Behavior

Scheduled jobs do not run while the user's machine is asleep.

Examples:
- laptop sleeping
- laptop lid closed
- suspended desktop state
- operating system sleep mode

If a scheduled execution time occurs while the machine is asleep:
- the job is missed
- the application does not attempt to retroactively execute the missed job
- and the next scheduled job runs normally

## Offline Behavior

If the machine:
- loses internet connectivity
- cannot reach APIs
- cannot access configured model providers
the scheduled job fails cleanly.

A failed job is different from a missed job.

A failed job occurs when the application successfully starts the job execution process but cannot complete the workflow because required services, APIs, or model providers are unavailable.

The application:
- cancels the current job
- stores the failure state
- displays a warning inside the Persistent Warning Area

## Concurrent Job Protection

Only one report-generation workflow may run at a time.

If a Weekly Market report job is currently running and another scheduled or manual execution is attempted, the second execution is skipped.

The application logs the skipped execution.

## Job Status Visibility

The application displays:
- last successful run time
- currently running job state, with live per-step and per-request progress in the run tracker (see [run-tracking.md](run-tracking.md))
- last failure state
- last cancelled run
- skipped job events

## Job Controls

Users can:
- Enable Weekly Market Job
- Disable Weekly Market Job

The Weekly Market Job is enabled by default.

Users can also **cancel a running job** at any point from the run tracker. Cancellation is cooperative and the run is recorded as a Cancelled job — see [run-tracking.md §Cancellation](run-tracking.md#cancellation).

The execution gate that prevents jobs from running until all required agent models and provider credentials are configured lives in [configuration.md](configuration.md).

## Manual Report Generation

The application includes manual execution controls in Settings for:
- Weekly Market Report

Manual execution follows the same workflow and validation rules as scheduled execution.

## Error Handling

If a job fails because of:
- API limits
- token exhaustion
- provider failures
- malformed responses
- model execution errors
the application:
1. cleanly cancels the job
2. stores the failure state
3. displays a warning inside the Persistent Warning Area

The Persistent Warning Area de-duplicates warnings within each category. See [interface.md §Persistent Warning Area](interface.md#persistent-warning-area) for the canonical rule.

## Missed Job Detection

The application detects when a scheduled job was missed because:
- the application was not running
- the machine was asleep
- the application could not start the scheduled execution during the scheduled window

Missed jobs are different from failed jobs.

A missed job means the scheduled execution never started.
A failed job means execution started but could not complete successfully.

When the application is next opened or resumed, it displays a warning inside the Persistent Warning Area indicating that the scheduled job was missed.

The user may:
- dismiss the warning
- manually execute the missed job immediately

Missed jobs are not automatically replayed or queued.
