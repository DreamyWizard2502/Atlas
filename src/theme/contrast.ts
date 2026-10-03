// WCAG 2.x contrast math plus the list of foreground/background pairs the UI
// actually renders. The palette test checks every pair in every palette.
import type { Tokens } from "./palettes.ts";

export function isHex(c: string): boolean {
  return /^#[0-9a-f]{6}$/i.test(c);
}

function channel(v: number): number {
  const s = v / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

export function luminance(hex: string): number {
  const n = parseInt(hex.slice(1), 16);
  return 0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255);
}

export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** HSL hue (0-360) and saturation (0-1). */
export function hueSat(hex: string): { hue: number; sat: number } {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  const l = (max + min) / 2;
  const sat = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  let hue = 0;
  if (d !== 0) {
    if (max === r) hue = 60 * (((g - b) / d) % 6);
    else if (max === g) hue = 60 * ((b - r) / d + 2);
    else hue = 60 * ((r - g) / d + 4);
  }
  return { hue: (hue + 360) % 360, sat };
}

export const AA_TEXT = 4.5;
export const AA_NON_TEXT = 3;

type Key = keyof Tokens;
export interface Pair {
  fg: Key;
  bg: Key;
  min: number;
  use: string;
}

const pairs = (fgs: Key[], bgs: Key[], min: number, use: string): Pair[] =>
  fgs.flatMap((fg) => bgs.map((bg) => ({ fg, bg, min, use })));

/** Every place text (or a meaningful mark) is drawn on a palette color. */
export const CONTRAST_PAIRS: Pair[] = [
  // Body, secondary and muted text on page, cards, hover rows, selection and "kept" rows.
  ...pairs(["text", "text2", "text3"], ["bg", "surface", "surface2", "accentSoft", "greenSoft"], AA_TEXT, "text"),
  ...pairs(["accent"], ["bg", "surface", "surface2", "accentSoft"], AA_TEXT, "accent text, active nav, accent pills"),
  ...pairs(["amber"], ["bg", "surface", "surface2", "amberSoft"], AA_TEXT, "needs-attention text and pills"),
  ...pairs(["green"], ["bg", "surface", "surface2", "greenSoft"], AA_TEXT, "reclaimable / cleaned text"),
  ...pairs(["danger"], ["bg", "surface", "surface2"], AA_TEXT, "destructive actions"),
  ...pairs(["cold"], ["bg", "surface", "surface2"], AA_TEXT, "cold-file labels"),
  ...pairs(["onAccent"], ["accent", "accentStrong"], AA_TEXT, "primary button text (rest and hover)"),
  ...pairs(["onSuccess"], ["greenMark"], AA_TEXT, "success button text"),
  ...pairs(["text", "text2"], ["accentTile", "fileTile", "coldTile"], AA_TEXT, "treemap tile labels"),
  // Graphical objects that carry meaning: bars, meters, timeline dots.
  ...pairs(["accent", "amberMark", "greenMark"], ["surface"], AA_NON_TEXT, "meaningful marks"),
];
