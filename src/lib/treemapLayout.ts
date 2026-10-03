// Treemap geometry, kept free of React and the DOM so it can be unit-tested
// with Node's test runner.
import { formatCount } from "./format.ts";
import type { TreeNode } from "./api.ts";


export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

function worst(row: number[], side: number): number {
  const sum = row.reduce((a, b) => a + b, 0);
  const max = Math.max(...row);
  const min = Math.min(...row);
  const s2 = side * side;
  return Math.max((s2 * max) / (sum * sum), (sum * sum) / (s2 * min));
}

export function squarify(values: number[], box: Rect): Rect[] {
  const total = values.reduce((a, b) => a + b, 0);
  if (total <= 0 || box.w <= 0 || box.h <= 0) return values.map(() => ({ x: 0, y: 0, w: 0, h: 0 }));
  const scale = (box.w * box.h) / total;
  const areas = values.map((v) => v * scale);
  const out: Rect[] = [];
  let { x, y, w, h } = box;
  let i = 0;
  while (i < areas.length) {
    const side = Math.min(w, h);
    const row: number[] = [areas[i]];
    let j = i + 1;
    while (j < areas.length && worst([...row, areas[j]], side) <= worst(row, side)) {
      row.push(areas[j]);
      j++;
    }
    const sum = row.reduce((a, b) => a + b, 0);
    if (w >= h) {
      const cw = sum / h;
      let cy = y;
      for (const a of row) {
        const ch = a / cw;
        out.push({ x, y: cy, w: cw, h: ch });
        cy += ch;
      }
      x += cw;
      w -= cw;
    } else {
      const rh = sum / w;
      let cx = x;
      for (const a of row) {
        const cw = a / rh;
        out.push({ x: cx, y, w: cw, h: rh });
        cx += cw;
      }
      y += rh;
      h -= rh;
    }
    i = j;
  }
  return out;
}

/** Smallest tile side (px) that's comfortable to hover and click. */
export const MIN_TILE = 30;

export interface Tile {
  node: TreeNode;
  /** Set for the "Other: N items" tile: the entries folded into it. */
  group?: TreeNode[];
}

const entriesOf = (n: TreeNode) => n.entries ?? 1;

/** Width of the strip reserved for the "Other" tile (enough for its label). */
const GROUP_STRIP = 92;

/** Fold the smallest entries into one "Other" tile until every tile is at
 * least MIN_TILE on both sides. "Other" gets its own full-height strip at the
 * right (full-width at the bottom in tall boxes), so it is always clickable no
 * matter how small its share is. */
export function layoutTiles(nodes: TreeNode[], w: number, h: number): { tiles: Tile[]; rects: Rect[] } {
  const sorted = nodes.filter((n) => n.size > 0).sort((a, b) => b.size - a.size);
  const total = sorted.reduce((a, n) => a + n.size, 0);
  const attempt = (k: number) => {
    const kept = sorted.slice(0, k);
    const rest = sorted.slice(k);
    const tiles: Tile[] = kept.map((node) => ({ node }));
    let box: Rect = { x: 0, y: 0, w, h };
    let groupRect: Rect | null = null;
    if (rest.length) {
      const items = rest.reduce((a, n) => a + entriesOf(n), 0);
      const size = rest.reduce((a, n) => a + n.size, 0);
      tiles.push({
        node: {
          path: "",
          name: `Other: ${formatCount(items)} items`,
          size,
          isDir: false,
          kind: "other",
          fileCount: rest.reduce((a, n) => a + n.fileCount, 0),
          other: true,
          category: "other",
          categoryShare: 0,
          entries: items,
        },
        group: rest,
      });
      const share = total > 0 ? size / total : 0;
      if (!kept.length) {
        groupRect = { x: 0, y: 0, w, h };
      } else if (w >= h) {
        const gw = Math.min(w / 2, Math.max(GROUP_STRIP, w * share));
        groupRect = { x: w - gw, y: 0, w: gw, h };
        box = { x: 0, y: 0, w: w - gw, h };
      } else {
        const gh = Math.min(h / 2, Math.max(MIN_TILE * 1.5, h * share));
        groupRect = { x: 0, y: h - gh, w, h: gh };
        box = { x: 0, y: 0, w, h: h - gh };
      }
    }
    const rects = squarify(
      kept.map((n) => n.size),
      box,
    );
    if (groupRect) rects.push(groupRect);
    const ok = rects.slice(0, kept.length).every((r) => r.w >= MIN_TILE && r.h >= MIN_TILE);
    return { tiles, rects, ok };
  };
  let k = sorted.length;
  let res = attempt(k);
  while (!res.ok && k > 0) {
    k = Math.max(0, k - Math.max(1, Math.ceil(k * 0.15)));
    res = attempt(k);
  }
  // Give back as many tiles as still fit.
  while (k < sorted.length) {
    const next = attempt(k + 1);
    if (!next.ok) break;
    k++;
    res = next;
  }
  return { tiles: res.tiles, rects: res.rects };
}

