import assert from "node:assert/strict";
import { test } from "node:test";
import { PALETTES, paletteById } from "./palettes.ts";
import { activePalette, pickPalette } from "./resolve.ts";
import type { ThemeState } from "./resolve.ts";

const base: ThemeState = { mode: "system", light: "linen", dark: "midnight", preview: null, osDark: false };
const byId = (id: string) => PALETTES.find((p) => p.id === id)!;

test("Match Windows switches between the two chosen palettes", () => {
  assert.equal(activePalette({ ...base, osDark: false }).id, "linen");
  assert.equal(activePalette({ ...base, osDark: true }).id, "midnight");
});

test("forced modes ignore Windows", () => {
  assert.equal(activePalette({ ...base, mode: "light", osDark: true }).id, "linen");
  assert.equal(activePalette({ ...base, mode: "dark", osDark: false }).id, "midnight");
});

test("picking a palette of the scheme in use applies it directly", () => {
  const s = { ...base, osDark: true };
  const patch = pickPalette(s, byId("abyss"));
  assert.deepEqual(patch, { dark: "abyss", preview: null });
  assert.equal(activePalette({ ...s, ...patch }).id, "abyss");
});

test("picking a palette of the other scheme previews it instantly", () => {
  const s = { ...base, osDark: false }; // Windows is light
  const patch = pickPalette(s, byId("plum"));
  assert.deepEqual(patch, { dark: "plum", preview: "dark" });
  assert.equal(activePalette({ ...s, ...patch }).id, "plum");
  // Ending the preview goes back to the light pick, with plum kept for dark mode.
  assert.equal(activePalette({ ...s, ...patch, preview: null }).id, "linen");
  assert.equal(activePalette({ ...s, ...patch, preview: null, osDark: true }).id, "plum");
});

test("unknown or mismatched palette ids fall back to the scheme default", () => {
  assert.equal(paletteById("nope", "dark").id, "graphite");
  assert.equal(paletteById("midnight", "light").id, "paper");
  assert.equal(activePalette({ ...base, light: "deleted-palette" }).id, "paper");
});
