// Hand-rolled charts. Treemap tiles are colored by storage category from the
// palette's category tokens (kept clear of the amber/green/red meaning colors);
// the growth line uses the single accent.
import { useMemo, useState } from "react";
import type { Category, GrowthPoint, TreeNode } from "../lib/api";
import { formatBytes, formatCount } from "../lib/format";
import { layoutTiles } from "../lib/treemapLayout";
export { MIN_TILE, layoutTiles, squarify } from "../lib/treemapLayout";
import { useSize } from "../lib/hooks";
import { openMenu, select } from "../lib/store";
import { FileIcon } from "./FileIcon";
import { activate } from "./FileViews";

/** Storage categories: label and palette token (see classify.rs). */
export const CATEGORY_INFO: Record<Category, { label: string; color: string }> = {
  games: { label: "Games and apps", color: "var(--cat-games)" },
  cad: { label: "CAD and 3D", color: "var(--cat-cad)" },
  documents: { label: "Documents", color: "var(--cat-documents)" },
  media: { label: "Media", color: "var(--cat-media)" },
  code: { label: "Code and projects", color: "var(--cat-code)" },
  archives: { label: "Archives and installers", color: "var(--cat-archives)" },
  temp: { label: "Temp and cache", color: "var(--cat-temp)" },
  other: { label: "Other", color: "var(--cat-other)" },
};

function tooltipLines(n: TreeNode, total: number): string[] {
  const pct = total > 0 ? ` · ${((n.size / total) * 100).toFixed(1)}%` : "";
  const lines = [`${formatBytes(n.size)}${pct}`];
  if (n.other) lines.push(`${formatCount(n.entries)} items · ${formatCount(n.fileCount)} files`);
  else if (n.isDir) lines.push(`${formatCount(n.fileCount)} files`);
  const cat = CATEGORY_INFO[n.category] ?? CATEGORY_INFO.other;
  if (!n.other) lines.push(n.isDir && n.categoryShare > 0 && n.categoryShare < 0.995 ? `Mostly ${cat.label.toLowerCase()} (${Math.round(n.categoryShare * 100)}%)` : cat.label);
  return lines;
}

export function Treemap({
  nodes,
  total,
  onZoom,
  onOpenGroup,
}: {
  nodes: TreeNode[];
  total: number;
  onZoom: (path: string) => void;
  onOpenGroup: (items: TreeNode[], label: string) => void;
}) {
  const [ref, size] = useSize<HTMLDivElement>();
  const [hover, setHover] = useState<{ node: TreeNode; x: number; y: number } | null>(null);
  const { tiles, rects } = useMemo(() => layoutTiles(nodes, size.width, size.height), [nodes, size.width, size.height]);
  const refs = tiles.filter((t) => !t.node.other).map((t) => toFileRef(t.node));
  return (
    <div ref={ref} className="relative h-full w-full" onMouseLeave={() => setHover(null)}>
      {tiles.map((t, i) => {
        const r = rects[i];
        if (!r || r.w < 1 || r.h < 1) return null;
        const n = t.node;
        const showLabel = r.w > 64 && r.h > 30;
        const ref = toFileRef(n);
        return (
          <div
            key={n.path || `other-${i}`}
            className="absolute overflow-hidden rounded-[4px] transition-[filter] duration-100 hover:brightness-[0.94] dark:hover:brightness-125"
            style={{
              left: r.x + 1,
              top: r.y + 1,
              width: Math.max(0, r.w - 2),
              height: Math.max(0, r.h - 2),
              background: (CATEGORY_INFO[n.category] ?? CATEGORY_INFO.other).color,
            }}
            onMouseMove={(e) => {
              const box = (e.currentTarget.parentElement as HTMLElement).getBoundingClientRect();
              setHover({ node: n, x: e.clientX - box.left, y: e.clientY - box.top });
            }}
            onClick={() => {
              if (t.group) onOpenGroup(t.group, n.name);
              else if (n.other) return;
              else if (n.isDir) onZoom(n.path);
              else select(ref, refs);
            }}
            onDoubleClick={() => !n.other && !n.isDir && activate(ref)}
            onContextMenu={(e) => !n.other && openMenu(e, [ref])}
          >
            {showLabel && (
              <div className="px-2 py-1.5 leading-tight text-ink">
                {t.group ? (
                  <>
                    <div className="truncate text-xs font-medium">Other</div>
                    <div className="tabular truncate text-[11px] text-ink-2">{formatCount(n.entries)} items</div>
                    {r.h > 58 && <div className="tabular truncate text-[11px] text-ink-2">{formatBytes(n.size)}</div>}
                  </>
                ) : (
                  <>
                    <div className="truncate text-xs font-medium">{n.name}</div>
                    {r.h > 46 && <div className="tabular truncate text-[11px] text-ink-2">{formatBytes(n.size)}</div>}
                  </>
                )}
              </div>
            )}
          </div>
        );
      })}
      {hover && (
        <div
          className="pointer-events-none absolute z-10 min-w-[170px] rounded-lg border border-line bg-surface px-3 py-2 text-xs shadow-[var(--shadow)]"
          style={{
            left: Math.min(hover.x + 14, Math.max(0, size.width - 230)),
            top: hover.y + 16 > size.height - 92 ? Math.max(0, hover.y - 92) : hover.y + 16,
          }}
        >
          <div className="mb-1 max-w-[260px] truncate font-medium text-ink">{hover.node.name}</div>
          {tooltipLines(hover.node, total).map((l) => (
            <div key={l} className="tabular text-ink-2">
              {l}
            </div>
          ))}
          <div className="mt-1 text-ink-3">
            {hover.node.other ? (hover.node.path ? "" : "Click to list them") : hover.node.isDir ? "Click to zoom in" : "Double-click to open"}
          </div>
        </div>
      )}
    </div>
  );
}

function toFileRef(n: TreeNode) {
  return { path: n.path, name: n.name, kind: n.kind, isDir: n.isDir, size: n.size };
}

/** Legend for the categories present, largest first. */
export function TreemapLegend({ nodes }: { nodes: TreeNode[] }) {
  const bytes = new Map<Category, number>();
  for (const n of nodes) bytes.set(n.category, (bytes.get(n.category) ?? 0) + n.size);
  const present = [...bytes.entries()].sort((a, b) => b[1] - a[1]);
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-ink-3">
      {present.map(([c]) => (
        <span key={c} className="flex items-center gap-1.5">
          <span className="h-2.5 w-2.5 rounded-[3px]" style={{ background: CATEGORY_INFO[c].color }} />
          {CATEGORY_INFO[c].label}
        </span>
      ))}
    </div>
  );
}

/** Sorted list alternative to the treemap (and the "Other" tile's contents). */
export function StorageList({ nodes, total, onZoom }: { nodes: TreeNode[]; total: number; onZoom: (path: string) => void }) {
  const sorted = [...nodes].sort((a, b) => b.size - a.size);
  const max = Math.max(1, ...sorted.map((n) => n.size));
  const refs = sorted.filter((n) => !n.other).map(toFileRef);
  return (
    <div className="h-full overflow-auto">
      {sorted.map((n, i) => {
        const cat = CATEGORY_INFO[n.category] ?? CATEGORY_INFO.other;
        const ref = toFileRef(n);
        return (
          <div
            key={n.path || `rest-${i}`}
            onClick={() => (n.other ? undefined : n.isDir ? onZoom(n.path) : select(ref, refs))}
            onDoubleClick={() => !n.other && !n.isDir && activate(ref)}
            onContextMenu={(e) => !n.other && openMenu(e, [ref])}
            title={tooltipLines(n, total).join("\n")}
            className="grid grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_72px] items-center gap-3 rounded-lg px-2 py-1.5 hover:bg-surface-2"
          >
            <div className="flex min-w-0 items-center gap-2">
              <span className="h-2.5 w-2.5 shrink-0 rounded-[3px]" style={{ background: cat.color }} />
              {n.other ? <span className="w-4" /> : <FileIcon kind={n.kind} size={14} />}
              <span className="truncate text-[13px]">{n.name}</span>
            </div>
            <div className="flex min-w-0 items-center gap-2">
              <div className="h-1.5 flex-1 rounded-full bg-surface-2">
                <div className="h-1.5 rounded-full bg-accent" style={{ width: `${Math.max(1, (n.size / max) * 100)}%` }} />
              </div>
              <span className="w-24 shrink-0 truncate text-[11px] text-ink-3">{n.isDir ? `${formatCount(n.fileCount)} files` : n.other ? `${formatCount(n.entries)} items` : cat.label}</span>
            </div>
            <span className="tabular text-right text-xs text-ink-2">{formatBytes(n.size)}</span>
          </div>
        );
      })}
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

/** Growth line. Estimated days (from file dates) are drawn dashed and lighter;
 * real daily snapshots are solid. */
export function GrowthChart({ points }: { points: GrowthPoint[] }) {
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
  const pathOf = (from: number, to: number) =>
    points
      .slice(from, to + 1)
      .map((p, j) => `${j ? "L" : "M"}${xs(from + j).toFixed(1)},${ys(p.size).toFixed(1)}`)
      .join("");
  const firstReal = points.findIndex((p) => !p.estimated);
  const lastEst = firstReal === -1 ? points.length - 1 : firstReal; // dashed segment joins the first real point
  const last = points.length - 1;
  const full = pathOf(0, last);
  const area = `${full}L${xs(last).toFixed(1)},${pad.t + ih}L${xs(0).toFixed(1)},${pad.t + ih}Z`;
  const ticks: number[] = [];
  for (let v = lo; v <= top + 1; v += step) ticks.push(v);
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
          {[0, Math.floor(last / 2), last]
            .filter((v, i, a) => a.indexOf(v) === i)
            .map((i) => (
              <text key={i} x={xs(i)} y={H - 6} textAnchor={i === 0 ? "start" : i === last ? "end" : "middle"} fontSize={10.5} fill="var(--text-3)">
                {shortDay(points[i].day)}
              </text>
            ))}
          <path d={area} fill="var(--accent)" opacity={0.1} />
          {firstReal !== 0 && (
            <path d={pathOf(0, lastEst)} fill="none" stroke="var(--accent)" strokeWidth={2} strokeDasharray="4 4" opacity={0.6} strokeLinecap="round" />
          )}
          {firstReal !== -1 && <path d={pathOf(firstReal, last)} fill="none" stroke="var(--accent)" strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />}
          {h != null && <line x1={xs(h)} x2={xs(h)} y1={pad.t} y2={pad.t + ih} stroke="var(--border-strong)" strokeWidth={1} />}
          <circle cx={xs(h ?? last)} cy={ys(points[h ?? last].size)} r={4} fill="var(--accent)" stroke="var(--surface)" strokeWidth={2} />
        </svg>
      )}
      {h != null && (
        <div
          className="pointer-events-none absolute rounded-lg border border-line bg-surface px-2.5 py-1.5 text-xs shadow-[var(--shadow)]"
          style={{ left: Math.min(xs(h) + 10, W - 170), top: 4 }}
        >
          <div className="text-ink-3">
            {shortDay(points[h].day)}
            {points[h].estimated ? " · estimated" : ""}
          </div>
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
