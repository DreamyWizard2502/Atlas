// Pure theme resolution, separate from the DOM so it can be unit-tested.
import { paletteById } from "./palettes.ts";
import type { Palette, Scheme } from "./palettes.ts";

export type ThemeMode = "system" | "light" | "dark";

export interface ThemeState {
  mode: ThemeMode;
  light: string;
  dark: string;
  /** Temporarily show the other scheme's palette (Settings live preview). */
  preview: Scheme | null;
  osDark: boolean;
}

/** The scheme chosen by the mode, ignoring any preview. */
export function baseScheme(s: ThemeState): Scheme {
  return s.mode === "system" ? (s.osDark ? "dark" : "light") : s.mode;
}

export function effectiveScheme(s: ThemeState): Scheme {
  return s.preview ?? baseScheme(s);
}

export function activePalette(s: ThemeState): Palette {
  const scheme = effectiveScheme(s);
  return paletteById(scheme === "dark" ? s.dark : s.light, scheme);
}

/**
 * State change for clicking a palette card: it becomes the pick for its scheme
 * and shows immediately. If its scheme isn't the one in use, it's previewed.
 */
export function pickPalette(s: ThemeState, p: Palette): Partial<ThemeState> {
  const patch = p.scheme === "dark" ? { dark: p.id } : { light: p.id };
  return { ...patch, preview: p.scheme === baseScheme({ ...s, ...patch }) ? null : p.scheme };
}
