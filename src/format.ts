// Shared display formatters. Kept in one place so the report-date convention has
// a single home — the earlier per-component duplication is what let the date
// drift between surfaces.

// Render an ISO-8601 (UTC) instant as its `YYYY-MM-DD` date in the user's local
// time zone — the calendar date that matches their wall clock. Reports are
// local-time artifacts (the job runs "Sunday 9:00 AM local"; docs/scheduling.md),
// and the backend already names report files by local date
// (pipeline::canonical_report_filename / export_basename via chrono::Local), so
// deriving the displayed date the same way keeps the sidebar/toolbar datelines,
// the Markdown export name, and the PDF export name in agreement. A naive
// `iso.slice(0, 10)` would instead show the UTC calendar date, which can be a
// day off near midnight.
export function localDate(iso: string): string {
  const d = new Date(iso);
  const year = d.getFullYear();
  const month = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
