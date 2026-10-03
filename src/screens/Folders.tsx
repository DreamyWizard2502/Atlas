// Classic view: folder tree, tabs, dropdown breadcrumbs, file list, details pane.
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  ChevronDown,
  ChevronRight,
  Eye,
  EyeOff,
  Folder,
  HardDrive,
  Plus,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { FileIcon, kindLabel } from "../components/FileIcon";
import { FileTable, sortFiles, type FileLike, type SortKey } from "../components/FileViews";
import { Button, Empty, IconButton, Pill, Spinner } from "../components/ui";
import { api, type DirEntry, type FileDetails, type Place } from "../lib/api";
import { errorMessage, openFile, revealFile } from "../lib/actions";
import { formatBytes, formatCount, formatDate, plural } from "../lib/format";
import { useStore, setState, type FileRef } from "../lib/store";

interface Tab {
  id: number;
  history: string[];
  index: number;
}

let tabSeq = 0;
// Tabs survive switching screens.
let savedTabs: { tabs: Tab[]; active: number } | null = null;

function splitPath(p: string): { name: string; path: string }[] {
  const norm = p.replace(/\//g, "\\");
  const m = norm.match(/^([A-Za-z]:\\?|\\\\[^\\]+\\[^\\]+\\?)/);
  const root = m ? (m[1].endsWith("\\") ? m[1] : m[1] + "\\") : "";
  const rest = norm.slice(m ? m[1].length : 0).split("\\").filter(Boolean);
  const out: { name: string; path: string }[] = [];
  let acc = root;
  if (root) out.push({ name: root.replace(/\\$/, ""), path: root });
  for (const seg of rest) {
    acc = acc && !acc.endsWith("\\") ? `${acc}\\${seg}` : `${acc}${seg}`;
    out.push({ name: seg, path: acc });
  }
  return out;
}

function parentOf(p: string): string | null {
  const parts = splitPath(p);
  return parts.length > 1 ? parts[parts.length - 2].path : null;
}

function baseName(p: string) {
  const parts = splitPath(p);
  return parts[parts.length - 1]?.name ?? p;
}

export function Folders() {
  const requested = useStore((s) => s.folderPath);
  const status = useStore((s) => s.status);
  const [places, setPlaces] = useState<Place[]>([]);
  const [tabs, setTabs] = useState<Tab[]>(() => savedTabs?.tabs ?? []);
  const [active, setActive] = useState<number>(() => savedTabs?.active ?? 0);

  useEffect(() => {
    api.places().then(setPlaces).catch(() => {});
  }, []);

  useEffect(() => {
    savedTabs = { tabs, active };
  }, [tabs, active]);

  const opened = useRef(false);
  const tab = tabs.find((t) => t.id === active) ?? tabs[0];
  const path = tab ? tab.history[tab.index] : null;

  const go = (p: string, opts: { newTab?: boolean } = {}) => {
    const cur = tabs.find((t) => t.id === active) ?? tabs[0];
    if (opts.newTab || !cur) {
      const t = { id: ++tabSeq, history: [p], index: 0 };
      setTabs((ts) => [...ts, t]);
      setActive(t.id);
      return;
    }
    if (cur.history[cur.index] === p) return;
    setActive(cur.id);
    setTabs((ts) => ts.map((t) => (t.id === cur.id ? { ...t, history: [...t.history.slice(0, t.index + 1), p], index: t.index + 1 } : t)));
  };

  // External navigation requests (double-clicking a folder anywhere in the app).
  useEffect(() => {
    if (requested) {
      go(requested);
      setState({ folderPath: null });
    } else if (!tabs.length && !opened.current) {
      const start = status?.home ?? places[0]?.path;
      if (start) {
        opened.current = true;
        go(start, { newTab: true });
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requested, places.length, status?.home]);

  const step = (d: number) =>
    setTabs((ts) => ts.map((t) => (t.id === tab?.id ? { ...t, index: Math.max(0, Math.min(t.history.length - 1, t.index + d)) } : t)));

  const closeTab = (id: number) => {
    const rest = tabs.filter((t) => t.id !== id);
    setTabs(rest);
    if (id === active && rest.length) setActive(rest[rest.length - 1].id);
  };

  return (
    <div className="flex h-full">
      <aside className="flex w-[240px] shrink-0 flex-col border-r border-line bg-surface">
        <div className="min-h-0 flex-1 overflow-auto px-2 py-3">
          <div className="px-2 pb-1 text-[11px] font-medium text-ink-3">Places</div>
          {places
            .filter((p) => p.kind !== "drive")
            .map((p) => (
              <TreeItem key={p.path} name={p.name} path={p.path} depth={0} current={path} onOpen={go} />
            ))}
          <div className="px-2 pb-1 pt-3 text-[11px] font-medium text-ink-3">Drives</div>
          {places
            .filter((p) => p.kind === "drive")
            .map((p) => (
              <TreeItem key={p.path} name={p.name} path={p.path} depth={0} current={path} onOpen={go} drive />
            ))}
        </div>
      </aside>
      <section className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-end gap-1 border-b border-line bg-surface px-3 pt-2">
          {tabs.map((t) => {
            const p = t.history[t.index];
            const isActive = t.id === tab?.id;
            return (
              <div
                key={t.id}
                onClick={() => setActive(t.id)}
                onAuxClick={(e) => e.button === 1 && tabs.length > 1 && closeTab(t.id)}
                className={`group flex h-8 max-w-[220px] items-center gap-2 rounded-t-lg border border-b-0 px-3 text-[12.5px] ${
                  isActive ? "border-line bg-bg text-ink" : "border-transparent text-ink-3 hover:text-ink"
                }`}
              >
                <Folder size={13} className={isActive ? "text-accent" : ""} />
                <span className="truncate">{baseName(p)}</span>
                {tabs.length > 1 && (
                  <button
                    type="button"
                    aria-label="Close tab"
                    className="rounded p-0.5 opacity-0 hover:bg-surface-2 group-hover:opacity-100"
                    onClick={(e) => {
                      e.stopPropagation();
                      closeTab(t.id);
                    }}
                  >
                    <X size={12} />
                  </button>
                )}
              </div>
            );
          })}
          <IconButton title="New tab" className="mb-0.5" onClick={() => path && go(path, { newTab: true })}>
            <Plus size={14} />
          </IconButton>
        </div>
        {path && tab ? (
          <FolderPane
            key={tab.id}
            path={path}
            canBack={tab.index > 0}
            canForward={tab.index < tab.history.length - 1}
            onBack={() => step(-1)}
            onForward={() => step(1)}
            onGo={go}
            home={status?.home}
          />
        ) : (
          <Empty title="Pick a place on the left" />
        )}
      </section>
    </div>
  );
}

function TreeItem({
  name,
  path,
  depth,
  current,
  onOpen,
  drive,
}: {
  name: string;
  path: string;
  depth: number;
  current: string | null;
  onOpen: (p: string) => void;
  drive?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [kids, setKids] = useState<{ name: string; path: string }[] | null>(null);
  const isCurrent = current?.toLowerCase() === path.toLowerCase();
  const toggle = async () => {
    if (!open && !kids) {
      try {
        setKids(await api.listSubdirs(path));
      } catch {
        setKids([]);
      }
    }
    setOpen(!open);
  };
  return (
    <div>
      <div
        className={`flex h-7 items-center gap-1 rounded-md pr-2 text-[13px] ${isCurrent ? "bg-accent-soft text-ink" : "text-ink-2 hover:bg-surface-2"}`}
        style={{ paddingLeft: 4 + depth * 14 }}
        onClick={() => onOpen(path)}
      >
        <button
          type="button"
          aria-label={open ? "Collapse" : "Expand"}
          className="flex h-5 w-5 shrink-0 items-center justify-center rounded text-ink-3 hover:text-ink"
          onClick={(e) => {
            e.stopPropagation();
            toggle();
          }}
        >
          {kids?.length === 0 ? null : open ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
        </button>
        {drive ? <HardDrive size={14} className="shrink-0 text-ink-3" /> : <Folder size={14} className="shrink-0 text-accent" />}
        <span className="truncate">{name}</span>
      </div>
      {open &&
        kids?.map((k) => <TreeItem key={k.path} name={k.name} path={k.path} depth={depth + 1} current={current} onOpen={onOpen} />)}
    </div>
  );
}

function FolderPane({
  path,
  canBack,
  canForward,
  onBack,
  onForward,
  onGo,
  home,
}: {
  path: string;
  canBack: boolean;
  canForward: boolean;
  onBack: () => void;
  onForward: () => void;
  onGo: (p: string) => void;
  home?: string | null;
}) {
  const [entries, setEntries] = useState<DirEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  const [filter, setFilter] = useState("");
  const [sort, setSort] = useState<{ key: SortKey; dir: "asc" | "desc" }>({ key: "name", dir: "asc" });
  const [nonce, setNonce] = useState(0);
  const dataVersion = useStore((s) => s.dataVersion);
  const selection = useStore((s) => s.selection);

  useEffect(() => {
    let alive = true;
    setError(null);
    api
      .listDir(path)
      .then((e) => alive && setEntries(e))
      .catch((e) => alive && (setError(errorMessage(e)), setEntries([])));
    return () => {
      alive = false;
    };
  }, [path, nonce, dataVersion]);

  useEffect(() => {
    setFilter("");
    setEntries(null);
  }, [path]);

  const items: FileLike[] = useMemo(() => {
    const f = filter.trim().toLowerCase();
    const list = (entries ?? [])
      .filter((e) => showHidden || !e.hidden)
      .filter((e) => !f || e.name.toLowerCase().includes(f))
      .map((e) => ({ path: e.path, name: e.name, kind: e.kind, ext: e.ext, isDir: e.isDir, size: e.size, modified: e.modified, parent: path }));
    return sortFiles(list, sort.key, sort.dir);
  }, [entries, showHidden, filter, sort, path]);

  const onSort = (k: SortKey) => setSort((s) => (s.key === k ? { key: k, dir: s.dir === "asc" ? "desc" : "asc" } : { key: k, dir: k === "name" ? "asc" : "desc" }));
  const parent = parentOf(path);
  const hiddenCount = (entries ?? []).filter((e) => e.hidden).length;
  const inFolder = selection && parentOf(selection.path)?.toLowerCase() === path.toLowerCase() ? selection : null;

  return (
    <div className="flex min-h-0 flex-1">
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-1 border-b border-line px-3 py-2">
          <IconButton title="Back" disabled={!canBack} onClick={onBack}>
            <ArrowLeft size={15} />
          </IconButton>
          <IconButton title="Forward" disabled={!canForward} onClick={onForward}>
            <ArrowRight size={15} />
          </IconButton>
          <IconButton title="Up one level" disabled={!parent} onClick={() => parent && onGo(parent)}>
            <ArrowUp size={15} />
          </IconButton>
          <Breadcrumbs path={path} onGo={onGo} />
          <div className="relative">
            <Search size={13} className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-ink-3" />
            <input
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter"
              className="h-7 w-40 rounded-md border border-line bg-surface pl-7 pr-2 text-xs outline-none focus:border-accent"
            />
          </div>
          <IconButton title={showHidden ? "Hide hidden files" : `Show hidden files (${hiddenCount})`} active={showHidden} onClick={() => setShowHidden(!showHidden)}>
            {showHidden ? <Eye size={15} /> : <EyeOff size={15} />}
          </IconButton>
          <IconButton title="Refresh" onClick={() => setNonce((n) => n + 1)}>
            <RefreshCw size={14} />
          </IconButton>
        </div>
        <div className="min-h-0 flex-1">
          {entries == null ? (
            <div className="flex h-full items-center justify-center">
              <Spinner />
            </div>
          ) : (
            <FileTable
              items={items}
              sort={sort}
              onSort={onSort}
              home={home}
              emptyState={<Empty title={error ? "Can't open this folder" : filter ? "No matches" : "This folder is empty"} body={error ?? undefined} />}
            />
          )}
        </div>
        <div className="border-t border-line px-4 py-1.5 text-[11.5px] text-ink-3">
          {plural(items.length, "item")}
          {!showHidden && hiddenCount > 0 && ` · ${hiddenCount} hidden`}
        </div>
      </div>
      <DetailsPane file={inFolder} />
    </div>
  );
}

function Breadcrumbs({ path, onGo }: { path: string; onGo: (p: string) => void }) {
  const segs = splitPath(path);
  const [menu, setMenu] = useState<{ index: number; items: { name: string; path: string }[] | null } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setMenu(null);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menu]);

  const openSiblings = async (i: number) => {
    if (menu?.index === i) return setMenu(null);
    const parent = i > 0 ? segs[i - 1].path : null;
    setMenu({ index: i, items: null });
    try {
      const items = parent ? await api.listSubdirs(parent) : (await api.places()).filter((p) => p.kind === "drive").map((p) => ({ name: p.name, path: p.path }));
      setMenu((m) => (m?.index === i ? { index: i, items } : m));
    } catch {
      setMenu((m) => (m?.index === i ? { index: i, items: [] } : m));
    }
  };

  return (
    <div ref={ref} className="relative mx-1 flex min-w-0 flex-1 items-center overflow-hidden rounded-md border border-line bg-surface px-1">
      {segs.map((s, i) => (
        <div key={s.path} className={`flex min-w-0 items-center ${i < segs.length - 3 ? "shrink" : "shrink-0"}`}>
          <button type="button" onClick={() => onGo(s.path)} className={`h-7 truncate rounded px-1.5 text-[12.5px] hover:bg-surface-2 ${i === segs.length - 1 ? "font-medium text-ink" : "text-ink-2"}`}>
            {s.name}
          </button>
          <button
            type="button"
            title="Sibling folders"
            onClick={() => openSiblings(i)}
            className={`flex h-7 w-4 shrink-0 items-center justify-center rounded text-ink-3 hover:bg-surface-2 hover:text-ink ${menu?.index === i ? "bg-surface-2 text-ink" : ""}`}
          >
            <ChevronDown size={11} />
          </button>
        </div>
      ))}
      {menu && (
        <div className="anim-fade fixed z-40 mt-1 max-h-80 min-w-[200px] overflow-auto rounded-xl border border-line bg-surface p-1 shadow-[var(--shadow)]" style={menuPos(ref.current, menu.index)}>
          {menu.items == null ? (
            <div className="p-3">
              <Spinner />
            </div>
          ) : menu.items.length === 0 ? (
            <div className="px-3 py-2 text-xs text-ink-3">No folders</div>
          ) : (
            menu.items.map((it) => (
              <button
                key={it.path}
                type="button"
                onClick={() => {
                  setMenu(null);
                  onGo(it.path);
                }}
                className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-surface-2 ${
                  it.path.toLowerCase() === segs[menu.index].path.toLowerCase() ? "font-medium text-accent" : ""
                }`}
              >
                <Folder size={13} className="shrink-0 text-accent" />
                <span className="truncate">{it.name}</span>
              </button>
            ))
          )}
        </div>
      )}
    </div>
  );
}

function menuPos(container: HTMLDivElement | null, index: number): React.CSSProperties {
  const btn = container?.querySelectorAll("button[title='Sibling folders']")[index] as HTMLElement | undefined;
  if (!btn) return {};
  const r = btn.getBoundingClientRect();
  return { left: Math.min(r.left - 8, window.innerWidth - 240), top: r.bottom + 2 };
}

function DetailsPane({ file }: { file: FileRef | null }) {
  const [d, setD] = useState<FileDetails | null>(null);
  const dataVersion = useStore((s) => s.dataVersion);
  useEffect(() => {
    setD(null);
    if (!file) return;
    let alive = true;
    api.fileDetails(file.path).then((x) => alive && setD(x)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [file?.path, dataVersion]);

  return (
    <aside className="w-[260px] shrink-0 overflow-auto border-l border-line bg-surface p-4">
      {!file ? (
        <p className="pt-8 text-center text-xs text-ink-3">Select a file to see its details. Press Space for Quick Look.</p>
      ) : (
        <div className="space-y-4 text-xs">
          <div className="flex flex-col items-center gap-2 pt-2 text-center">
            <FileIcon kind={file.kind} size={40} />
            <div className="selectable break-all text-[13px] font-medium">{file.name}</div>
            <div className="text-ink-3">{kindLabel(file.kind, file.ext)}</div>
          </div>
          <div className="flex justify-center gap-2">
            {!file.isDir && (
              <Button size="sm" onClick={() => openFile(file)}>
                Open
              </Button>
            )}
            <Button size="sm" onClick={() => revealFile(file)}>
              Reveal
            </Button>
          </div>
          {d ? (
            <dl className="space-y-2.5">
              <Row label="Size">
                {formatBytes(d.size)}
                {d.fileCount != null && ` · ${formatCount(d.fileCount)} files`}
                {d.isDir && d.fileCount == null && <span className="text-ink-3"> (not indexed)</span>}
              </Row>
              <Row label="Modified">{formatDate(d.modified)}</Row>
              <Row label="Created">{formatDate(d.created)}</Row>
              <Row label="Accessed">{formatDate(d.accessed)}</Row>
              {(d.readonly || d.hidden) && (
                <Row label="Attributes">
                  {[d.readonly && "Read-only", d.hidden && "Hidden"].filter(Boolean).join(", ")}
                </Row>
              )}
              <Row label="Tags">
                <div className="flex flex-wrap items-center gap-1">
                  {d.tags.map((t) => (
                    <span key={t} className="group inline-flex items-center gap-0.5">
                      <Pill tone="accent">
                        {t}
                        <button
                          type="button"
                          aria-label={`Remove tag ${t}`}
                          onClick={async () => {
                            await api.removeTag(d.path, t);
                            setD({ ...d, tags: d.tags.filter((x) => x !== t) });
                          }}
                          className="opacity-60 hover:opacity-100"
                        >
                          <X size={10} />
                        </button>
                      </Pill>
                    </span>
                  ))}
                  <button type="button" className="text-accent hover:underline" onClick={() => setState({ dialog: { kind: "tag", files: [file] } })}>
                    Add
                  </button>
                </div>
              </Row>
              {d.spaces.length > 0 && (
                <Row label="In spaces">
                  <div className="flex flex-wrap gap-1">
                    {d.spaces.map((s) => (
                      <Pill key={s}>{s}</Pill>
                    ))}
                  </div>
                </Row>
              )}
            </dl>
          ) : (
            <Spinner />
          )}
        </div>
      )}
    </aside>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="mb-0.5 text-[11px] text-ink-3">{label}</dt>
      <dd className="text-ink">{children}</dd>
    </div>
  );
}
