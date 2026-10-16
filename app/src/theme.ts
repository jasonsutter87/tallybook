// Theme choice (system / light / dark), kept per viewer in localStorage.
// Lives outside Settings so startup can apply it without loading that page.

export type ThemeChoice = "system" | "light" | "dark";
const THEME_KEY = "tallybook.theme";

export function readTheme(): ThemeChoice {
  try {
    const v = localStorage.getItem(THEME_KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

function applyTheme(t: ThemeChoice) {
  const root = document.documentElement;
  if (t === "system") delete root.dataset.theme;
  else root.dataset.theme = t;
}

/** Call once at startup (main.tsx) so the saved theme applies before first paint. */
export function applySavedTheme() {
  applyTheme(readTheme());
}

export function saveTheme(t: ThemeChoice) {
  applyTheme(t);
  try {
    if (t === "system") localStorage.removeItem(THEME_KEY);
    else localStorage.setItem(THEME_KEY, t);
  } catch {
    /* storage unavailable: the choice still applies for this session */
  }
}
