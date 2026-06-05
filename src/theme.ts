// Light/Dark appearance preference — the design kit's "Dark surface" switch.
// The dark token set lives in colors_and_type.css under `:root[data-theme="dark"]`;
// this module is the only thing that toggles that attribute, so flipping it
// cascades the whole token-driven UI at once.
//
// Persisted in localStorage on purpose: it is synchronous, so main.ts can apply
// the stored choice *before* Vue mounts and the first paint never flashes the
// wrong theme. Appearance is deliberately kept out of the token-gated
// `save_settings` flow — it has no backend consumer (agents never see HTML; PDF
// export reuses this same DOM and inherits the theme), and a backend round-trip
// would reintroduce the launch flash this avoids.

const STORAGE_KEY = "appearance";

/** The stored choice, defaulting to light when unset or unreadable. */
export function readDark(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "dark";
  } catch {
    return false;
  }
}

/** Reflect the choice onto <html> so the dark token block cascades (or not). */
export function applyTheme(dark: boolean): void {
  const root = document.documentElement;
  if (dark) {
    root.setAttribute("data-theme", "dark");
  } else {
    root.removeAttribute("data-theme");
  }
}

/** Persist the choice and apply it in one step. */
export function writeDark(dark: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, dark ? "dark" : "light");
  } catch {
    // Best-effort persistence — apply regardless so the toggle still works this
    // session even when storage is unavailable.
  }
  applyTheme(dark);
}
