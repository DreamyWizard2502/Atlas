// Run with `npm test` (Node's built-in test runner; Node 22.6+ runs TypeScript directly).
import assert from "node:assert/strict";
import { test } from "node:test";
import { AA_TEXT, CATEGORY_TOKENS, CONTRAST_PAIRS, contrast, deltaE, hueSat, isHex } from "./contrast.ts";
import { DEFAULT_DARK, DEFAULT_LIGHT, PALETTES } from "./palettes.ts";
import type { Palette, Tokens } from "./palettes.ts";

const dark = PALETTES.filter((p) => p.scheme === "dark");
const light = PALETTES.filter((p) => p.scheme === "light");
const COLOR_KEYS = (Object.keys(PALETTES[0].tokens) as (keyof Tokens)[]).filter((k) => k !== "overlay" && k !== "shadow");

test("20 palettes: 10 dark, 10 light, unique ids and names", () => {
  assert.equal(dark.length, 10);
  assert.equal(light.length, 10);
  assert.equal(new Set(PALETTES.map((p) => p.id)).size, 20);
  assert.equal(new Set(PALETTES.map((p) => p.name)).size, 20);
  assert.equal(PALETTES.find((p) => p.id === DEFAULT_DARK)?.scheme, "dark");
  assert.equal(PALETTES.find((p) => p.id === DEFAULT_LIGHT)?.scheme, "light");
});

test("every color token is a 6-digit hex", () => {
  for (const p of PALETTES) for (const k of COLOR_KEYS) assert.ok(isHex(p.tokens[k]), `${p.id}.${k} = ${p.tokens[k]}`);
});

for (const p of PALETTES) {
  test(`${p.name}: every text/background pair meets WCAG AA`, () => {
    const failures = CONTRAST_PAIRS.map((pair) => ({ ...pair, ratio: contrast(p.tokens[pair.fg], p.tokens[pair.bg]) }))
      .filter((r) => r.ratio < r.min)
      .map((r) => `${r.fg} ${p.tokens[r.fg]} on ${r.bg} ${p.tokens[r.bg]} = ${r.ratio.toFixed(2)} (needs ${r.min}, ${r.use})`);
    assert.deepEqual(failures, []);
  });
}

const BACKGROUNDS: (keyof Tokens)[] = ["bg", "surface", "surface2", "accentSoft", "amberSoft", "greenSoft", ...CATEGORY_TOKENS];
const TEXTS: (keyof Tokens)[] = ["text", "text2", "text3", "accent", "amber", "green", "danger", "cold", "onAccent", "onSuccess"];

test("dark palettes: no pure black backgrounds, no pure white text", () => {
  for (const p of dark) {
    for (const k of BACKGROUNDS) assert.notEqual(p.tokens[k].toLowerCase(), "#000000", `${p.id}.${k}`);
    for (const k of TEXTS) assert.notEqual(p.tokens[k].toLowerCase(), "#ffffff", `${p.id}.${k}`);
    // Dimmed text: comfortably readable, but not maximum contrast.
    assert.ok(contrast(p.tokens.text, p.tokens.bg) < 17, `${p.id} text is too harsh`);
  }
});

test("light palettes: soft off-white backgrounds, never pure white", () => {
  for (const p of light) {
    for (const k of ["bg", "surface"] as const) assert.notEqual(p.tokens[k].toLowerCase(), "#ffffff", `${p.id}.${k}`);
    assert.ok(contrast("#ffffff", p.tokens.bg) > 1.04, `${p.id} background is too close to pure white`);
  }
});

function hueDistance(a: number, b: number): number {
  const d = Math.abs(a - b) % 360;
  return d > 180 ? 360 - d : d;
}

const inRange = (hue: number, from: number, to: number) => (from <= to ? hue >= from && hue <= to : hue >= from || hue <= to);

test("semantic colors keep their meaning in every palette", () => {
  const check = (p: Palette) => {
    const t = p.tokens;
    assert.ok(inRange(hueSat(t.amber).hue, 30, 60), `${p.id}: amber hue ${hueSat(t.amber).hue.toFixed(0)}`);
    assert.ok(inRange(hueSat(t.green).hue, 90, 170), `${p.id}: green hue ${hueSat(t.green).hue.toFixed(0)}`);
    assert.ok(inRange(hueSat(t.danger).hue, 330, 20), `${p.id}: danger hue ${hueSat(t.danger).hue.toFixed(0)}`);
    assert.ok(hueSat(t.cold).sat <= 0.15, `${p.id}: cold must stay a muted gray`);
    // The accent must not be mistaken for a meaning color.
    const accent = hueSat(t.accent);
    if (accent.sat > 0.2) {
      for (const k of ["amber", "green", "danger"] as const) {
        const d = hueDistance(accent.hue, hueSat(t[k]).hue);
        assert.ok(d >= 15, `${p.id}: accent too close to ${k} (${d.toFixed(0)}°)`);
      }
    }
  };
  PALETTES.forEach(check);
});

test("treemap categories: distinct, and never mistaken for meaning colors", () => {
  for (const p of PALETTES) {
    const t = p.tokens;
    const chromatic = CATEGORY_TOKENS.filter((k) => k !== "catTemp" && k !== "catOther");
    for (const k of chromatic) {
      const c = hueSat(t[k]);
      for (const sem of ["amber", "green", "danger"] as const) {
        const d = hueDistance(c.hue, hueSat(t[sem]).hue);
        assert.ok(d >= 15, `${p.id}: ${k} hue too close to ${sem} (${d.toFixed(0)}°)`);
      }
    }
    for (const k of ["catTemp", "catOther"] as const) assert.ok(hueSat(t[k]).sat <= 0.15, `${p.id}: ${k} must stay gray`);
    // Labels, legend and tooltips carry identity too, so 6 is the floor (8 is ideal).
    for (let i = 0; i < CATEGORY_TOKENS.length; i++) {
      for (let j = i + 1; j < CATEGORY_TOKENS.length; j++) {
        const [a, b] = [CATEGORY_TOKENS[i], CATEGORY_TOKENS[j]];
        const d = deltaE(t[a], t[b]);
        assert.ok(d >= 6, `${p.id}: ${a} and ${b} are too similar (dE ${d.toFixed(1)})`);
      }
    }
  }
});

test("AA threshold is the standard 4.5:1 for text", () => {
  assert.equal(AA_TEXT, 4.5);
});
