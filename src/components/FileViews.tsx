// Virtualized file list (table) and card grid. Both share the same selection,
// open, Quick Look and context-menu behavior.
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import { useMemo, useRef } from "react";
import { openFile } from "../lib/actions";
import { formatBytes, formatDate, relativeTime, shortPath } from "../lib/format";
import { useSize } from "../lib/hooks";
import { navigate, openMenu, select, useStore, type FileRef } from "../lib/store";
import { FileIcon, Thumb } from "./FileIcon";

export type FileLike = FileRef & { parent?: string; created?: number | null };

export function activate(f: FileRef) {
  if (f.isDir || f.kind === "folder") navigate("folders", { folderPath: f.path });
  else openFile(f);
}

export function fileHandlers(f: FileRef, list: FileRef[]) {
  return {
    onClick: () => select(f, list),
    onDoubleClick: () => activate(f),
    onContextMenu: (e: React.MouseEvent) => openMenu(e, [f]),
    "data-path": f.path,
  };
}

export type SortKey = "name" | "modified" | "size" | "kind";

export function sortFiles<T extends FileLike>(items: T[], key: SortKey, dir: "asc" | "desc", foldersFirst = true): T[] {
  const m = dir === "asc" ? 1 : -1;
  return [...items].sort((a, b) => {
    if (foldersFirst && !!a.isDir !== !!b.isDir) return a.isDir ? -1 : 1;
    let r = 0;
    if (key === "name") r = a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
    else if (key === "modified") r = (a.modified ?? 0) - (b.modified ?? 0);
    else if (key === "size") r = (a.size ?? -1) - (b.size ?? -1);
    else r = (a.ext ?? a.kind).localeCompare(b.ext ?? b.kind);
    return r * m;
  });
}

function HeaderCell({
  label,
  k,
  sort,
  onSort,
  className = "",
}: {
  label: string;
  k: SortKey;
  sort?: { key: SortKey; dir: "asc" | "desc" };
  onSort?: (k: SortKey) => void;
  className?: string;
}) {
  const active = sort?.key === k;
  return (
    <button
      type="button"
      disabled={!onSort}
      onClick={() => onSort?.(k)}
      className={`flex items-center gap-1 truncate text-left text-[11.5px] font-medium ${active ? "text-ink" : "text-ink-3"} ${onSort ? "hover:text-ink" : ""} ${className}`}
    >
      {label}
      {active && (sort!.dir === "asc" ? <ArrowUp size={11} /> : <ArrowDown size={11} />)}
    </button>
  );
}

export function FileTable({
  items,
  showLocation = false,
  sort,
  onSort,
  home,
  emptyState,
  rowExtra,
}: {
  items: FileLike[];
  showLocation?: boolean;
  sort?: { key: SortKey; dir: "asc" | "desc" };
  onSort?: (k: SortKey) => void;
  home?: string | null;
  emptyState?: React.ReactNode;
  rowExtra?: (f: FileLike) => React.ReactNode;
}) {
  const parentRef = useRef<HTMLDivElement>(null);
  const selected = useStore((s) => s.selection?.path);
  const v = useVirtualizer({ count: items.length, getScrollElement: () => parentRef.current, estimateSize: () => 34, overscan: 12 });
  const cols = showLocation ? "minmax(0,2.2fr) minmax(0,1.6fr) 140px 90px" : "minmax(0,3fr) 160px 110px 90px";
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="grid shrink-0 items-center gap-3 border-b border-line px-4 py-2" style={{ gridTemplateColumns: cols }}>
        <HeaderCell label="Name" k="name" sort={sort} onSort={onSort} />
        {showLocation ? <span className="text-[11.5px] font-medium text-ink-3">Location</span> : <HeaderCell label="Modified" k="modified" sort={sort} onSort={onSort} />}
        {showLocation ? <HeaderCell label="Modified" k="modified" sort={sort} onSort={onSort} /> : <HeaderCell label="Type" k="kind" sort={sort} onSort={onSort} />}
        <HeaderCell label="Size" k="size" sort={sort} onSort={onSort} className="justify-end" />
      </div>
      {items.length === 0 ? (
        <div className="flex-1">{emptyState}</div>
      ) : (
        <div ref={parentRef} className="min-h-0 flex-1 overflow-auto">
          <div style={{ height: v.getTotalSize(), position: "relative" }}>
            {v.getVirtualItems().map((row) => {
              const f = items[row.index];
              const isSel = f.path === selected;
              return (
                <div
                  key={f.path}
                  {...fileHandlers(f, items)}
                  className={`absolute left-0 right-0 grid items-center gap-3 px-4 text-[13px] ${
                    isSel ? "bg-accent-soft" : "hover:bg-surface-2"
                  }`}
                  style={{ top: row.start, height: row.size, gridTemplateColumns: cols }}
                >
                  <div className="flex min-w-0 items-center gap-2.5">
                    <FileIcon kind={f.kind} />
                    <span className="truncate">{f.name}</span>
                    {rowExtra?.(f)}
                  </div>
                  {showLocation ? (
                    <span className="truncate text-xs text-ink-3" title={f.parent}>
                      {shortPath(f.parent ?? "", home)}
                    </span>
                  ) : (
                    <span className="truncate text-xs text-ink-2" title={formatDate(f.modified)}>
                      {formatDate(f.modified)}
                    </span>
                  )}
                  {showLocation ? (
                    <span className="truncate text-xs text-ink-2" title={formatDate(f.modified)}>
                      {relativeTime(f.modified)}
                    </span>
                  ) : (
                    <span className="truncate text-xs text-ink-3">{f.isDir ? "Folder" : f.ext ? f.ext.toUpperCase() : "File"}</span>
                  )}
                  <span className="tabular text-right text-xs text-ink-2">{f.size == null ? "" : formatBytes(f.size)}</span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

export function FileGrid({ items, emptyState }: { items: FileLike[]; emptyState?: React.ReactNode }) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [sizeRef, size] = useSize<HTMLDivElement>();
  const selected = useStore((s) => s.selection?.path);
  const minCard = 168;
  const gap = 12;
  const cols = Math.max(1, Math.floor((size.width - 32 + gap) / (minCard + gap)));
  const rows = useMemo(() => {
    const out: FileLike[][] = [];
    for (let i = 0; i < items.length; i += cols) out.push(items.slice(i, i + cols));
    return out;
  }, [items, cols]);
  const v = useVirtualizer({ count: rows.length, getScrollElement: () => parentRef.current, estimateSize: () => 176 + gap, overscan: 4 });
  return (
    <div ref={sizeRef} className="h-full min-h-0">
      {items.length === 0 ? (
        emptyState
      ) : (
        <div ref={parentRef} className="h-full overflow-auto px-4 py-3">
          <div style={{ height: v.getTotalSize(), position: "relative" }}>
            {v.getVirtualItems().map((row) => (
              <div
                key={row.key}
                className="absolute left-0 right-0 grid"
                style={{ top: row.start, gap, gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` }}
              >
                {rows[row.index].map((f) => (
                  <FileCard key={f.path} f={f} list={items} selected={f.path === selected} />
                ))}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export function FileCard({ f, list, selected, meta }: { f: FileLike; list: FileRef[]; selected: boolean; meta?: React.ReactNode }) {
  return (
    <div
      {...fileHandlers(f, list)}
      className={`group flex h-[176px] flex-col overflow-hidden rounded-xl border transition-colors duration-100 ${
        selected ? "border-accent bg-accent-soft" : "border-line bg-surface hover:border-line-strong"
      }`}
    >
      <div className="h-[104px] shrink-0 overflow-hidden border-b border-line bg-surface-2">
        <Thumb path={f.path} ext={f.ext} kind={f.kind} size={f.size} />
      </div>
      <div className="flex min-h-0 flex-1 flex-col justify-center gap-0.5 px-3">
        <span className="truncate text-[13px] font-medium" title={f.name}>
          {f.name}
        </span>
        <span className="truncate text-[11.5px] text-ink-3">
          {meta ?? (
            <>
              {relativeTime(f.modified)}
              {f.size != null && !f.isDir ? ` · ${formatBytes(f.size)}` : ""}
            </>
          )}
        </span>
      </div>
    </div>
  );
}
