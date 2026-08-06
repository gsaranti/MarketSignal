// Unit tests for the pure ET session-dating helpers — the frontend mirror of
// the backend's `market_clock::et_date_of` / `over_age` pair. Run via
// `npm test` on Node's built-in runner (type-stripping import, no build step).

import { test } from "node:test";
import assert from "node:assert/strict";
import { etDateOf, etDayDiff } from "../src/etDate.ts";

test("an evening-ET instant dates to the prior ET day, not the UTC date", () => {
  // 2026-08-05 01:30 UTC = 2026-08-04 21:30 EDT.
  assert.equal(etDateOf("2026-08-05T01:30:00Z"), "2026-08-04");
  // Winter (EST, UTC-5): 2026-01-07 03:30 UTC = 2026-01-06 22:30 EST.
  assert.equal(etDateOf("2026-01-07T03:30:00Z"), "2026-01-06");
  // A daytime instant stays on its own date.
  assert.equal(etDateOf("2026-06-23T16:04:00Z"), "2026-06-23");
});

test("a date-only stamp is taken as-is; garbage is null", () => {
  assert.equal(etDateOf("2026-08-05"), "2026-08-05");
  assert.equal(etDateOf("soon"), null);
  assert.equal(etDateOf(""), null);
});

test("malformed and zoneless stamps degrade to the date prefix, like the backend", () => {
  // A malformed timestamp degrades to its date prefix rather than vanishing —
  // market_clock::et_date_of's exact fallback.
  assert.equal(etDateOf("2026-08-05T99:99:99Z"), "2026-08-05");
  // A zoneless timestamp is no RFC3339 instant: the backend's strict parse
  // rejects it, so the frontend must not read it as machine-local time.
  assert.equal(etDateOf("2026-08-05T23:30:00"), "2026-08-05");
  // A prefix that is not a real calendar date is still null — the backend's
  // NaiveDate parse rejects it, so the fallback must too.
  assert.equal(etDateOf("2026-99-99T12:00:00Z"), null);
  assert.equal(etDateOf("2026-99-99"), null);
  // An overflowed-but-parseable day: Date.parse would normalize Feb 30 to
  // March 2, where the backend rejects the instant and its prefix — null on
  // both sides, never a fabricated date.
  assert.equal(etDateOf("2026-02-30T12:00:00Z"), null);
});

test("exotic RFC3339 forms match the backend's chrono parse case-for-case", () => {
  // The same cases market_clock's exotic-forms test pins on the Rust side:
  // accepted variants convert (ET day — one earlier in the evening window)...
  assert.equal(etDateOf("2026-08-06 01:30:00+00:00"), "2026-08-05");
  assert.equal(etDateOf("2026-08-06t01:30:00+00:00"), "2026-08-05");
  assert.equal(etDateOf("2026-08-06T01:30:00z"), "2026-08-05");
  assert.equal(etDateOf("2026-08-06T01:30:00.123+00:00"), "2026-08-05");
  assert.equal(etDateOf("2026-08-06T01:30:60+00:00"), "2026-08-05");
  // ...and rejected times degrade to the date prefix, never a normalized
  // instant: Date.parse alone would read T24:00-06:00 as the next ET day.
  assert.equal(etDateOf("2026-08-05T24:00:00-06:00"), "2026-08-05");
  assert.equal(etDateOf("2026-08-06T25:00:00+00:00"), "2026-08-06");
  assert.equal(etDateOf("2026-08-06T01:60:00+00:00"), "2026-08-06");
  assert.equal(etDateOf("2026-08-06T01:30:61+00:00"), "2026-08-06");
  assert.equal(etDateOf("2026-08-06T01:30:00+0000"), "2026-08-06");
});

test("etDayDiff counts whole ET days, matching the engine's date-diff", () => {
  // Exactly 28 ET days — inside the carry window (`> 28` is the stale test).
  assert.equal(
    etDayDiff("2026-07-08T15:00:00Z", "2026-08-05T15:00:00Z"),
    28,
  );
  // The evening vintage belongs to the prior ET session: 29 days — stale.
  // A fractional-ms age (27.99... days) would have read it fresh.
  assert.equal(
    etDayDiff("2026-07-08T01:30:00Z", "2026-08-05T15:00:00Z"),
    29,
  );
  assert.equal(etDayDiff("soon", "2026-08-05T15:00:00Z"), null);
});

test("the diff is direction-signed and zero within one ET day", () => {
  assert.equal(
    etDayDiff("2026-08-04T13:00:00Z", "2026-08-05T01:30:00Z"),
    0, // both instants sit inside the ET day of the 4th
  );
  assert.equal(
    etDayDiff("2026-08-05T15:00:00Z", "2026-08-04T15:00:00Z"),
    -1,
  );
});
