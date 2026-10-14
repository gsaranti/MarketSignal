// Specs share module-level fixtures by reference across wrappers, so a test that
// mutates one in place throws (ESM strict mode) instead of leaking state into a
// later test. Vue's `reactive()` leaves a non-extensible target unproxied, so a
// frozen fixture renders exactly as a plain one; the return type stays `T`
// rather than `Readonly<T>` so call sites' static types are unchanged.
export function deepFreeze<T>(value: T): T {
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    for (const key of Object.keys(value)) {
      deepFreeze((value as Record<string, unknown>)[key]);
    }
    Object.freeze(value);
  }
  return value;
}
