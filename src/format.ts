// Shared display formatters. Kept in one place so the report-date convention has
// a single home — the earlier per-component duplication is what let the date
// drift between surfaces.

// Render an ISO-8601 (UTC) instant as its `YYYY-MM-DD` date in the user's local
// time zone — the calendar date that matches their wall clock. Reports are
// local-time artifacts (generated and dated in the user's local time;
// docs/scheduling.md), and the backend already names report files by local date
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

// Render an ISO-8601 (UTC) instant as `YYYY-MM-DD HH:MM` in the user's local time
// zone — the local calendar date plus 24-hour wall-clock time. Used on the report
// datelines (sidebar rows, the toolbar) so two reports generated on the same day
// are distinguishable; the date-only `localDate` still backs the export filename,
// whose 8-char id already disambiguates same-day files.
export function localDateTime(iso: string): string {
  const d = new Date(iso);
  const hours = String(d.getHours()).padStart(2, "0");
  const minutes = String(d.getMinutes()).padStart(2, "0");
  return `${localDate(iso)} ${hours}:${minutes}`;
}
