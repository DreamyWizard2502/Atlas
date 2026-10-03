// Hand-rolled charts. Color carries meaning only: folders use the accent
// family, files a neutral tone, "everything else" the muted/cold tone.
import { useMemo, useState } from "react";
import type { TreeNode } from "../lib/api";
import { formatBytes, formatCount } from "../lib/format";
import { useSize } from "../lib/hooks";
import { openMenu, select } from "../lib/store";
import { activate } from "./FileViews";

// ---------------- Treemap (squarified) ----------------

interface Rect {
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

export function Treemap({ nodes, total, onZoom }: { nodes: TreeNode[]; total: number; onZoom: (path: string) => void }) {
  const [ref, size] = useSize<HTMLDivElement>();
  const [hover, setHover] = useState<{ node: TreeNode; x: number; y: number } | null>(null);
  const visible = useMemo(() => nodes.filter((n) => n.size > 0), [nodes]);
  const rects = useMemo(
    () => squarify(visible.map((n) => n.size), { x: 0, y: 0, w: size.width, h: size.height }),
    [visible, size.width, size.height],
  );
  const refs = visible.filter((n) => !n.other).map((n) => ({ path: n.path, name: n.name, kind: n.kind, isDir: n.isDir, size: n.size }));
  return (
    <div ref={ref} className="relative h-full w-full" onMouseLeave={() => setHover(null)}>
      {visible.map((n, i) => {
        const r = rects[i];
        if (!r || r.w < 1 || r.h < 1) return null;
        const bg = n.other ? "var(--cold-tile)" : n.isDir ? "var(--accent-tile)" : "var(--file-tile)";
        const showLabel = r.w > 64 && r.h > 30;
        const ref = { path: n.path, name: n.name, kind: n.kind, isDir: n.isDir, size: n.size };
        return (
          <div
            key={n.path || `other-${i}`}
            className="absolute overflow-hidden rounded-[4px] transition-[filter] duration-100 hover:brightness-[0.94] dark:hover:brightness-125"
            style={{ left: r.x + 1, top: r.y + 1, width: Math.max(0, r.w - 2), height: Math.max(0, r.h - 2), background: bg }}
            onMouseMove={(e) => {
              const box = (e.currentTarget.parentElement as HTMLElement).getBoundingClientRect();
              setHover({ node: n, x: e.clientX - box.left, y: e.clientY - box.top });
            }}
            onClick={() => {
              if (n.other) return;
              if (n.isDir) onZoom(n.path);
              else select(ref, refs);
            }}
            onDoubleClick={() => !n.other && !n.isDir && activate(ref)}
            onContextMenu={(e) => !n.other && openMenu(e, [ref])}
          >
            {showLabel && (
              <div className={`px-2 py-1.5 leading-tight ${n.other ? "text-ink-2" : "text-ink"}`}>
                <div className="truncate text-xs font-medium">{n.name}</div>
                {r.h > 46 && <div className="tabular truncate text-[11px] text-ink-2">{formatBytes(n.size)}</div>}
              </div>
            )}
          </div>
        );
      })}
      {hover && (
        <div
          className="pointer-events-none absolute z-10 min-w-[160px] rounded-lg border border-line bg-surface px-3 py-2 text-xs shadow-[var(--shadow)]"
          style={{
            left: Math.min(hover.x + 14, Math.max(0, size.width - 200)),
            top: hover.y + 16 > size.height - 70 ? hover.y - 74 : hover.y + 16,
          }}
        >
          <div className="mb-1 max-w-[240px] truncate font-medium text-ink">{hover.node.name}</div>
          <div className="tabular text-ink-2">
            {formatBytes(hover.node.size)} · {total > 0 ? ((hover.node.size / total) * 100).toFixed(1) : "0"}%
          </div>
          {(hover.node.isDir || hover.node.other) && <div className="tabular text-ink-3">{formatCount(hover.node.fileCount)} files</div>}
          {hover.node.isDir && <div className="mt-1 text-ink-3">Click to zoom in</div>}
        </div>
      )}
    </div>
  );
}

export function TreemapLegend() {
  const items = [
    ["var(--accent-tile)", "Folder"],
    ["var(--file-tile)", "File"],
    ["var(--cold-tile)", "Everything else"],
  ];
  return (
    <div className="flex items-center gap-3 text-[11px] text-ink-3">
      {items.map(([c, l]) => (
        <span key={l} className="flex items-center gap-1.5">
          <span className="h-2.5 w-2.5 rounded-[3px]" style={{ background: c }} />
          {l}
        </span>
      ))}
    </div>
  );
}

// ---------------- Growth line chart ----------------

function niceStep(range: number, ticks: number) {
  const raw = range / ticks;
  const mag = Math.pow(1024, Math.floor(Math.log(Math.max(raw, 1)) / Math.log(1024)));
  const norm = raw / mag;
  const nice = norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 2.5 ? 2.5 : norm <= 5 ? 5 : 10;
  return nice * mag;
}

function shortDay(day: string) {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function GrowthChart({ points }: { points: { day: string; size: number }[] }) {
  const [ref, size] = useSize<HTMLDivElement>();
  const [hi, setHi] = useState<number | null>(null);
  const W = size.width;
  const H = size.height;
  const pad = { l: 56, r: 16, t: 12, b: 22 };
  const iw = Math.max(1, W - pad.l - pad.r);
  const ih = Math.max(1, H - pad.t - pad.b);

  const { lo, hi: top, step } = useMemo(() => {
    const vals = points.map((p) => p.size);
    const min = Math.min(...vals);
    const max = Math.max(...vals);
    const range = Math.max(max - min, max * 0.02, 1024 * 1024);
    const step = niceStep(range, 3);
    const lo = Math.floor((min - range * 0.1) / step) * step;
    const hi = Math.ceil((max + range * 0.1) / step) * step;
    return { lo: Math.max(0, lo), hi, step };
  }, [points]);

  const xs = (i: number) => pad.l + (points.length <= 1 ? iw / 2 : (i / (points.length - 1)) * iw);
  const ys = (v: number) => pad.t + ih - ((v - lo) / Math.max(1, top - lo)) * ih;
  const line = points.map((p, i) => `${i ? "L" : "M"}${xs(i).toFixed(1)},${ys(p.size).toFixed(1)}`).join("");
  const area = `${line}L${xs(points.length - 1).toFixed(1)},${pad.t + ih}L${xs(0).toFixed(1)},${pad.t + ih}Z`;
  const ticks: number[] = [];
  for (let v = lo; v <= top + 1; v += step) ticks.push(v);
  const last = points.length - 1;
  const h = hi ?? null;

  return (
    <div
      ref={ref}
      className="relative h-full w-full"
      onMouseMove={(e) => {
        const box = e.currentTarget.getBoundingClientRect();
        const x = e.clientX - box.left - pad.l;
        const i = Math.round((x / iw) * (points.length - 1));
        setHi(Math.max(0, Math.min(points.length - 1, i)));
      }}
      onMouseLeave={() => setHi(null)}
    >
      {W > 0 && (
        <svg width={W} height={H} className="block">
          {ticks.map((t) => (
            <g key={t}>
              <line x1={pad.l} x2={W - pad.r} y1={ys(t)} y2={ys(t)} stroke="var(--border)" strokeWidth={1} />
              <text x={pad.l - 8} y={ys(t)} dy="0.32em" textAnchor="end" fontSize={10.5} fill="var(--text-3)" className="tabular">
                {formatBytes(t, 0)}
              </text>
            </g>
          ))}
          {[0, Math.floor(last / 2), last].filter((v, i, a) => a.indexOf(v) === i).map((i) => (
            <text key={i} x={xs(i)} y={H - 6} textAnchor={i === 0 ? "start" : i === last ? "end" : "middle"} fontSize={10.5} fill="var(--text-3)">
              {shortDay(points[i].day)}
            </text>
          ))}
          <path d={area} fill="var(--accent)" opacity={0.1} />
          <path d={line} fill="none" stroke="var(--accent)" strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
          {h != null && (
            <line x1={xs(h)} x2={xs(h)} y1={pad.t} y2={pad.t + ih} stroke="var(--border-strong)" strokeWidth={1} />
          )}
          <circle
            cx={xs(h ?? last)}
            cy={ys(points[h ?? last].size)}
            r={4}
            fill="var(--accent)"
            stroke="var(--surface)"
            strokeWidth={2}
          />
        </svg>
      )}
      {h != null && (
        <div
          className="pointer-events-none absolute rounded-lg border border-line bg-surface px-2.5 py-1.5 text-xs shadow-[var(--shadow)]"
          style={{ left: Math.min(xs(h) + 10, W - 150), top: 4 }}
        >
          <div className="text-ink-3">{shortDay(points[h].day)}</div>
          <div className="tabular font-medium text-ink">{formatBytes(points[h].size)}</div>
          {h > 0 && (
            <div className="tabular text-ink-3">
              {points[h].size - points[0].size >= 0 ? "+" : ""}
              {formatBytes(points[h].size - points[0].size)} since {shortDay(points[0].day)}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Horizontal bars for reclaimable space per category (single series, green = reclaimable). */
export function ReclaimBars({ rows, onClick }: { rows: { id: string; label: string; value: number; count: number }[]; onClick?: (id: string) => void }) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  return (
    <div className="space-y-1">
      {rows.map((r) => (
        <button
          key={r.id}
          type="button"
          onClick={() => onClick?.(r.id)}
          className="group grid w-full grid-cols-[minmax(0,1fr)_72px] items-center gap-3 rounded-lg px-2 py-1.5 text-left hover:bg-surface-2"
          title={`${r.label}: ${formatBytes(r.value)} in ${formatCount(r.count)} items`}
        >
          <div className="min-w-0">
            <div className="mb-1 flex items-baseline justify-between gap-2 text-xs">
              <span className="truncate text-ink">{r.label}</span>
              <span className="tabular shrink-0 text-ink-3">{formatCount(r.count)}</span>
            </div>
            <div className="h-1.5 rounded-full bg-surface-2">
              <div
                className="h-1.5 rounded-full bg-green-mark transition-[width] duration-300"
                style={{ width: `${r.value > 0 ? Math.max(2, (r.value / max) * 100) : 0}%` }}
              />
            </div>
          </div>
          <span className="tabular text-right text-xs font-medium text-ink">{formatBytes(r.value)}</span>
        </button>
      ))}
    </div>
  );
}
