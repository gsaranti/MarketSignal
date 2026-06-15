// deepFreeze — recursively freeze a fixture and everything it transitively
// references, returning the same object.
//
// Component specs share module-level fixtures and spread them *shallowly* into
// props (or return them from the mocked `invoke`), so nested objects/arrays stay
// shared by reference across every wrapper. Those fixtures are read-only by
// design; freezing turns that from a convention into a guarantee — a test that
// mutates one in place throws at the mutation site (spec modules are ESM, so
// strict mode) instead of silently leaking state into a later test.
//
// Safe for Vue props/stored values: `reactive()` returns a non-extensible target
// untouched (getTargetType → INVALID), so a frozen object is never proxied and
// renders exactly as a plain one. The returned `T` (not `Readonly<T>`) keeps call
// sites' static types identical — this only adds runtime immutability.
export function deepFreeze<T>(value: T): T {
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    for (const key of Object.keys(value)) {
      deepFreeze((value as Record<string, unknown>)[key]);
    }
    Object.freeze(value);
  }
  return value;
}
