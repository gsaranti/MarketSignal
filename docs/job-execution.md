# Job Execution and Scheduling

The application uses local scheduled jobs that run directly on the user's machine.

Jobs are responsible for:
- generating premarket reports
- generating postmarket reports
- generating the weekly review report

The job schedules and focus areas are defined in [scheduled-jobs.md](scheduled-jobs.md).

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
- the job is skipped
- the application does not attempt to retroactively execute the missed job
- and the next scheduled job runs normally

## Offline Behavior

If the machine:
- loses internet connectivity
- cannot reach APIs
- cannot access configured model providers
the scheduled job fails cleanly.

The application:
- cancels the current job
- stores the failure state
- displays a warning banner

## Concurrent Job Protection

Only one scheduled job may run at a time.

If a report job is currently running and another scheduled job time occurs, the second job is skipped.

The application logs the skipped execution.

## Job Controls

Users can:
- Enable Premarket Job
- Disable Premarket Job
- Enable Postmarket Job
- Disable Postmarket Job
- Enable Weekly Review Job
- Disable Weekly Review Job

By default, all are enabled.

## Manual Report Generation

The application includes manual execution controls for:
- Premarket Report
- Postmarket Report
- Weekly Review

Manual execution follows the same workflow and validation rules as scheduled execution.

## Job Status Visibility

The application displays:
- last successful run time
- currently running job state
- last failure state
- skipped job events

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
3. displays a warning banner inside the application

If the warning already exists and has not been dismissed/resolved:
- additional failing jobs do not create duplicate warnings.

## Missed Job Detection

The application detects when a scheduled job was missed because:
- the application was not running
- the machine was asleep
- the machine was offline during the scheduled execution window

When the application is next opened or resumed, it displays a notification indicating that the scheduled job was missed.

The user may:
- dismiss the notification
- manually execute the missed job immediately

Missed jobs are not automatically replayed or queued.
