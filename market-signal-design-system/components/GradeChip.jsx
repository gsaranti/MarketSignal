// GradeChip — a discrete tonal grade chip (A–F) from the unified analytical
// palette. Hairline/flat, never a glossy badge. Analytical register only.
// Reads --grade-{a..f}-tx / --grade-{a..f}-bg from colors_and_type.css.

const GRADE_KEY = { A: "a", B: "b", C: "c", D: "d", E: "f", F: "f" };

export function GradeChip({ value = "C", size = "md", style }) {
  const k = GRADE_KEY[String(value || "C")[0].toUpperCase()] || "c";
  const dims = size === "lg"
    ? { minWidth: 34, height: 30, fontSize: 18 }
    : size === "sm"
      ? { minWidth: 22, height: 19, fontSize: 12 }
      : { minWidth: 26, height: 22, fontSize: 14 };
  return (
    <span style={{
      display: "inline-flex", alignItems: "center", justifyContent: "center",
      padding: "0 6px", fontFamily: "var(--font-mono)", fontWeight: 600,
      lineHeight: 1, letterSpacing: 0,
      border: "1px solid var(--hairline)", borderRadius: 2,
      color: `var(--grade-${k}-tx)`, background: `var(--grade-${k}-bg)`,
      ...dims, ...style,
    }}>{value}</span>
  );
}
