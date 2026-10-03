import assert from "node:assert/strict";
import { test } from "node:test";
import type { TreeNode } from "./api.ts";
import { layoutTiles, MIN_TILE } from "./treemapLayout.ts";

const node = (name: string, size: number, extra: Partial<TreeNode> = {}): TreeNode => ({
  path: `C:\\x\\${name}`,
  name,
  size,
  isDir: true,
  kind: "folder",
  fileCount: 1,
  other: false,
  category: "other",
  categoryShare: 1,
  entries: 1,
  ...extra,
});

const W = 790;
const H = 330;

test("every tile is at least the clickable minimum", () => {
  // One giant folder and a long tail of tiny ones (a typical project or drive).
  const nodes = [node("src-tauri", 13.5e9), node("node_modules", 2e8), ...Array.from({ length: 60 }, (_, i) => node(`f${i}`, 1e5 + i))];
  const { tiles, rects } = layoutTiles(nodes, W, H);
  assert.equal(tiles.length, rects.length);
  for (const r of rects) assert.ok(r.w >= MIN_TILE && r.h >= MIN_TILE, JSON.stringify(r));
  // The giant stays its own tile; the tail is grouped, not dropped.
  assert.ok(tiles.some((t) => t.node.name === "src-tauri" && !t.group));
  const group = tiles.find((t) => t.group);
  assert.ok(group, "small items are grouped");
  assert.match(group!.node.name, /^Other: \d+ items$/);
  const shown = tiles.filter((t) => !t.group).length + group!.group!.length;
  assert.equal(shown, nodes.length, "nothing is lost");
  assert.equal(
    tiles.reduce((a, t) => a + t.node.size, 0),
    nodes.reduce((a, n) => a + n.size, 0),
  );
});

test("a single dominant item never swallows everything into Other", () => {
  const nodes = [node("huge", 1e12), ...Array.from({ length: 5 }, (_, i) => node(`tiny${i}`, 10))];
  const { tiles } = layoutTiles(nodes, W, H);
  assert.ok(tiles.some((t) => t.node.name === "huge" && !t.group));
});

test("balanced items need no grouping", () => {
  const nodes = Array.from({ length: 6 }, (_, i) => node(`n${i}`, 100 + i));
  const { tiles } = layoutTiles(nodes, W, H);
  assert.equal(tiles.length, 6);
  assert.ok(tiles.every((t) => !t.group));
});

test("the backend's 'more items' remainder counts toward the Other label", () => {
  const nodes = [node("big", 1e9), node("rest", 10, { other: true, entries: 340, isDir: false, kind: "other", path: "" })];
  const { tiles } = layoutTiles(nodes, W, H);
  const group = tiles.find((t) => t.group);
  assert.equal(group?.node.name, "Other: 340 items");
});

test("tall containers put Other along the bottom", () => {
  const nodes = [node("a", 1e9), ...Array.from({ length: 40 }, (_, i) => node(`t${i}`, 10))];
  const { rects, tiles } = layoutTiles(nodes, 300, 800);
  const gi = tiles.findIndex((t) => t.group);
  assert.ok(gi >= 0);
  assert.equal(rects[gi].w, 300);
  for (const r of rects) assert.ok(r.w >= MIN_TILE && r.h >= MIN_TILE);
});

test("empty and zero-size input is handled", () => {
  assert.deepEqual(layoutTiles([], W, H), { tiles: [], rects: [] });
  assert.equal(layoutTiles([node("zero", 0)], W, H).tiles.length, 0);
});
