// ET session dating for vintage timestamps — the frontend mirror of the
// backend's `market_clock::et_date_of` / `over_age` pair (src-tauri/src/
// market_clock.rs, portfolio/job.rs). Every persisted vintage is a UTC RFC3339
// instant, but the session it belongs to is its US/Eastern calendar day: an
// evening-ET run has already rolled to the next UTC date, so a UTC-based read
// dates it one session late. The stale/carried card tag compares whole ET
// days (date-diff), matching the engine's `over_age` boundary exactly —
// a fractional-milliseconds age would disagree with it around the boundary.

// en-CA formats as YYYY-MM-DD; the IANA zone handles DST like chrono-tz does.
const ET_DAY = new Intl.DateTimeFormat("en-CA", {
  timeZone: "America/New_York",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});

// A strict RFC3339 instant, mirroring exactly what the backend's chrono parse
// accepts (pinned by market_clock's exotic-forms test — same cases below in
// tests/etDate.test.ts): 'T'/'t'/space separator, hh 00–23, mm 00–59, ss 00–60
// (the :60 leap second), optional fraction, and a 'Z'/'z' or ±hh:mm offset.
// Everything else — a zoneless time (which Date.parse would read as
// machine-local), hour 24, minute 60, a colon-less offset — degrades to the
// prefix on both sides.
const RFC3339_INSTANT =
  /^\d{4}-\d{2}-\d{2}[Tt ](?:[01]\d|2[0-3]):[0-5]\d:(?:[0-5]\d|60)(?:\.\d+)?(?:[zZ]|[+-](?:[01]\d|2[0-3]):[0-5]\d)$/;

// A real calendar date, not just three digit groups — the backend's NaiveDate
// parse rejects "2026-99-99", so the prefix fallback must too.
function validDate(s: string): boolean {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return false;
  const d = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3]));
  return (
    d.getUTCFullYear() === +m[1] &&
    d.getUTCMonth() === +m[2] - 1 &&
    d.getUTCDate() === +m[3]
  );
}

/** The ET calendar date (YYYY-MM-DD) of an RFC3339 instant, or null when the
 * stamp does not parse. The backend helper's exact contract
 * (`market_clock::et_date_of`): a date-only stamp carries no instant to convert
 * and is taken as-is, and a malformed timestamp degrades to its date prefix
 * rather than vanishing. */
export function etDateOf(stamp: string): string | null {
  if (validDate(stamp)) return stamp;
  // The instant branch also requires a real calendar date in the date part:
  // Date.parse would normalize "2026-02-30T…Z" to March 2 where the backend's
  // strict RFC3339 parse rejects it outright.
  if (RFC3339_INSTANT.test(stamp) && validDate(stamp.slice(0, 10))) {
    // Canonicalize to the uppercase-'T' shape Date.parse handles per the ES
    // spec — the regex vouched for every field, so this cannot change the
    // instant — and fold a :60 leap second into :59 (the same ET day;
    // Date.parse rejects :60, chrono accepts it).
    const time = stamp
      .slice(11)
      .replace(/^(\d{2}:\d{2}:)60/, "$159")
      .toUpperCase();
    const ms = Date.parse(stamp.slice(0, 10) + "T" + time);
    if (Number.isFinite(ms)) return ET_DAY.format(new Date(ms));
  }
  const prefix = stamp.slice(0, 10);
  return validDate(prefix) ? prefix : null;
}

/** Whole ET days from `from` to `to` (positive when `to` is later) — the
 * date-diff the engine's `over_age` computes, never a fractional-ms age.
 * `null` when either stamp does not parse. */
export function etDayDiff(from: string, to: string): number | null {
  const a = etDateOf(from);
  const b = etDateOf(to);
  if (a === null || b === null) return null;
  // Both are YYYY-MM-DD; UTC-midnight parses make the difference exact.
  return Math.round((Date.parse(b) - Date.parse(a)) / 86_400_000);
}
