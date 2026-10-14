// Shared display formatters, so the report-date convention has a single home.

// The `YYYY-MM-DD` local-time date of a UTC ISO-8601 instant. Reports are dated
// in local time (docs/scheduling.md) and the backend names report files by local
// date, so the displayed date derives the same way; `iso.slice(0, 10)` would
// show the UTC date, a day off near midnight.
export function localDate(iso: string): string {
  const d = new Date(iso);
  const year = d.getFullYear();
  const month = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

// `YYYY-MM-DD HH:MM` (24-hour) in the user's local time zone.
export function localDateTime(iso: string): string {
  const d = new Date(iso);
  const hours = String(d.getHours()).padStart(2, "0");
  const minutes = String(d.getMinutes()).padStart(2, "0");
  return `${localDate(iso)} ${hours}:${minutes}`;
}
