// Light/Dark appearance preference. The dark token set in colors_and_type.css
// hangs off `:root[data-theme="dark"]`; this module is the only thing that
// toggles that attribute.
//
// Persisted in localStorage rather than through `save_settings` on purpose: it
// is synchronous, so main.ts applies the stored choice before Vue mounts and the
// first paint never flashes the wrong theme. Appearance has no backend consumer.

const STORAGE_KEY = "appearance";

/** The stored choice, defaulting to light when unset or unreadable. */
export function readDark(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "dark";
  } catch {
    return false;
  }
}

export function applyTheme(dark: boolean): void {
  const root = document.documentElement;
  if (dark) {
    root.setAttribute("data-theme", "dark");
  } else {
    root.removeAttribute("data-theme");
  }
}

export function writeDark(dark: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, dark ? "dark" : "light");
  } catch {
    // Storage may be unavailable; apply regardless so the toggle still works
    // this session.
  }
  applyTheme(dark);
}
