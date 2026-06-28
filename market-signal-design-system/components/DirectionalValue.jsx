// DirectionalValue — the up/down/flat treatment for the analytical register.
// Sign + weight + chevron + desaturated hue (muted-green up / oxblood down /
// neutral flat). Still no saturated red/green. Mono tabular figures.

const DIR_META = {
  up:   { color: "var(--ana-up)",   ch: "\u25B4" },
  down: { color: "var(--ana-down)", ch: "\u25BE" },
  flat: { color: "var(--ana-flat)", ch: "\u00B7" },
};

export function DirectionalValue({ dir = "flat", children, size = 13, style }) {
  const m = DIR_META[dir] || DIR_META.flat;
  return (
    <span style={{
      fontFamily: "var(--font-mono)",
      fontVariantNumeric: "tabular-nums lining-nums",
      fontWeight: 500, fontSize: size, letterSpacing: "-0.01em",
      color: m.color, display: "inline-flex", alignItems: "baseline", gap: 4,
      ...style,
    }}>
      <span aria-hidden="true">{m.ch}</span>
      <span>{children}</span>
    </span>
  );
}
