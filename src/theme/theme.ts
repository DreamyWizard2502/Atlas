// Runtime theming: resolves the active palette from the mode (Light, Dark or
// Match Windows) and the user's light/dark palette picks, and applies it as CSS
// variables on <html>. Every color in the UI comes from these variables.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSyncExternalStore } from "react";
import { DEFAULT_DARK, DEFAULT_LIGHT } from "./palettes.ts";
import type { Tokens } from "./palettes.ts";
import { activePalette } from "./resolve.ts";
import type { ThemeMode, ThemeState } from "./resolve.ts";

export { activePalette, baseScheme, effectiveScheme, pickPalette } from "./resolve.ts";
export type { ThemeMode, ThemeState } from "./resolve.ts";

const CSS_VARS: Record<keyof Tokens, string> = {
  bg: "--bg",
  surface: "--surface",
  surface2: "--surface-2",
  border: "--border",
  borderStrong: "--border-strong",
  text: "--text",
  text2: "--text-2",
  text3: "--text-3",
  accent: "--accent",
  accentStrong: "--accent-strong",
  accentSoft: "--accent-soft",
  onAccent: "--on-accent",
  accentTile: "--accent-tile",
  fileTile: "--file-tile",
  coldTile: "--cold-tile",
  amber: "--amber",
  amberMark: "--amber-mark",
  amberSoft: "--amber-soft",
  green: "--green",
  greenMark: "--green-mark",
  greenSoft: "--green-soft",
  onSuccess: "--on-success",
  danger: "--danger",
  cold: "--cold",
  overlay: "--overlay",
  shadow: "--shadow",
};

// Local cache so the first paint already uses the saved palette; the settings
// stored by the backend remain the source of truth.
const CACHE_KEY = "atlas.theme";
const media = window.matchMedia("(prefers-color-scheme: dark)");

function loadCached(): ThemeState {
  const base: ThemeState = { mode: "system", light: DEFAULT_LIGHT, dark: DEFAULT_DARK, preview: null, osDark: media.matches };
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (raw) {
      const c = JSON.parse(raw);
      return { ...base, mode: isMode(c.mode) ? c.mode : base.mode, light: c.light || base.light, dark: c.dark || base.dark };
    }
  } catch {
    // Storage unavailable: defaults are fine.
  }
  return base;
}

function isMode(m: unknown): m is ThemeMode {
  return m === "system" || m === "light" || m === "dark";
}

let state: ThemeState = loadCached();
const listeners = new Set<() => void>();
let lastWindowTheme: string | undefined;

export function getTheme(): ThemeState {
  return state;
}

function apply() {
  const p = activePalette(state);
  const root = document.documentElement;
  for (const [key, cssVar] of Object.entries(CSS_VARS) as [keyof Tokens, string][]) {
    root.style.setProperty(cssVar, p.tokens[key]);
  }
  root.style.colorScheme = p.scheme;
  root.dataset.scheme = p.scheme;
  root.dataset.palette = p.id;
  // Title bar follows the mode: Windows decides in Match Windows mode.
  const windowTheme = state.mode === "system" ? "auto" : state.mode;
  if (windowTheme !== lastWindowTheme) {
    lastWindowTheme = windowTheme;
    getCurrentWindow()
      .setTheme(windowTheme === "auto" ? null : windowTheme)
      .catch(() => {});
  }
}

export function setTheme(patch: Partial<ThemeState>) {
  state = { ...state, ...patch };
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify({ mode: state.mode, light: state.light, dark: state.dark }));
  } catch {
    // Not fatal; the backend settings still persist the choice.
  }
  apply();
  listeners.forEach((l) => l());
}

export function useTheme(): ThemeState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

/** Apply the cached theme before the first render and follow OS changes. */
export function initTheme() {
  apply();
  media.addEventListener("change", (e) => setTheme({ osDark: e.matches }));
}
