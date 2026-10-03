// Ctrl+K command palette: instant search over the index plus navigation commands.
import {
  Brush,
  Clock,
  CornerDownLeft,
  FolderTree,
  Layers,
  LayoutDashboard,
  RefreshCw,
  Search,
  Settings as SettingsIcon,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, type FileItem, type SearchResult, type Space } from "../lib/api";
import { revealFile, toRef } from "../lib/actions";
import { formatBytes, relativeTime, shortPath } from "../lib/format";
import { useDebounced } from "../lib/hooks";
import { navigate, openQuickLook, setState, toast, useStore } from "../lib/store";
import { activate } from "./FileViews";
import { FileIcon } from "./FileIcon";
import { Spinner } from "./ui";

interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: LucideIcon;
  run: () => void;
}

const FILTER_HINTS = [
  ["type:pdf", "by type — pdf, image, video, cad, doc, sheet, code, archive…"],
  ["ext:sldprt", "by extension, comma-separated"],
  ["size:>500mb", "larger or smaller than (kb, mb, gb)"],
  ["modified:this week", "today, yesterday, this week, last month, 30d…"],
  ["in:Client work", "inside a space (quote names with spaces)"],
];

export function CommandPalette() {
  const open = useStore((s) => s.palette);
  if (!open) return null;
  return <PaletteInner />;
}

function PaletteInner() {
  const home = useStore((s) => s.status?.home);
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const [res, setRes] = useState<SearchResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [spaces, setSpaces] = useState<Space[]>([]);
  const dq = useDebounced(q, 70);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const close = () => setState({ palette: false });

  useEffect(() => {
    inputRef.current?.focus();
    api.listSpaces().then(setSpaces).catch(() => {});
  }, []);

  const commands: Command[] = useMemo(
    () => [
      { id: "dash", label: "Go to dashboard", icon: LayoutDashboard, run: () => navigate("dashboard") },
      { id: "spaces", label: "Go to spaces", icon: Layers, run: () => navigate("spaces") },
      { id: "timeline", label: "Go to timeline", icon: Clock, run: () => navigate("timeline") },
      { id: "cleanup", label: "Go to cleanup", icon: Brush, run: () => navigate("cleanup") },
      { id: "folders", label: "Go to folders", icon: FolderTree, run: () => navigate("folders") },
      { id: "settings", label: "Open settings", icon: SettingsIcon, run: () => navigate("settings") },
      {
        id: "rescan",
        label: "Rescan indexed locations",
        icon: RefreshCw,
        run: () => api.startScan().then(() => toast("Rescan started")),
      },
      {
        id: "analyze",
        label: "Analyze cleanup opportunities",
        icon: Sparkles,
        run: () => {
          api.cleanupAnalyze();
          navigate("cleanup");
        },
      },
      ...spaces.map((s) => ({
        id: `space-${s.id}`,
        label: `Open space: ${s.name}`,
        hint: `${s.count} files`,
        icon: Layers,
        run: () => navigate("spaces", { spaceId: s.id }),
      })),
    ],
    [spaces],
  );

  const commandMode = q.startsWith(">");
  const cq = (commandMode ? q.slice(1) : q).trim().toLowerCase();
  const shownCommands = commandMode || !q.trim()
    ? commands.filter((c) => c.label.toLowerCase().includes(cq)).slice(0, commandMode ? 50 : 6)
    : commands.filter((c) => cq.length >= 2 && c.label.toLowerCase().includes(cq)).slice(0, 3);

  useEffect(() => {
    if (commandMode || !dq.trim()) {
      setRes(null);
      return;
    }
    let alive = true;
    setLoading(true);
    api
      .search(dq, 60)
      .then((r) => alive && (setRes(r), setActive(0)))
      .catch(() => alive && setRes(null))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [dq, commandMode]);

  const files: FileItem[] = res?.items ?? [];
  const total = shownCommands.length + files.length;

  const runIndex = (i: number, mod: "open" | "reveal" | "look") => {
    if (i < shownCommands.length) {
      shownCommands[i].run();
      close();
      return;
    }
    const f = files[i - shownCommands.length];
    if (!f) return;
    const ref = toRef(f);
    if (mod === "reveal") revealFile(ref);
    else if (mod === "look") {
      setState({ palette: false });
      openQuickLook(ref, files.map(toRef));
      return;
    } else activate(ref);
    close();
  };

  useEffect(() => {
    listRef.current?.querySelector(`[data-idx="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(total - 1, a + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      runIndex(active, e.ctrlKey ? "reveal" : e.shiftKey ? "look" : "open");
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  return (
    <div className="anim-fade fixed inset-0 z-50 flex justify-center bg-[var(--overlay)] px-6 pt-[12vh]" onMouseDown={close}>
      <div
        className="anim-pop card flex h-fit max-h-[70vh] w-full max-w-[680px] flex-col overflow-hidden shadow-[var(--shadow)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2.5 border-b border-line px-4">
          <Search size={16} className="text-ink-3" />
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={onKey}
            placeholder="Search files, or type > for commands"
            className="h-12 flex-1 bg-transparent text-[15px] text-ink outline-none placeholder:text-ink-3"
            spellCheck={false}
          />
          {loading && <Spinner />}
          {res && !loading && <span className="tabular text-[11px] text-ink-3">{res.tookMs} ms</span>}
        </div>
        {res && res.parsed.chips.length > 0 && (
          <div className="flex flex-wrap gap-1.5 border-b border-line px-4 py-2">
            {res.parsed.chips.map((c) => (
              <span key={c} className="rounded-full bg-accent-soft px-2 py-0.5 text-[11px] font-medium text-accent">
                {c}
              </span>
            ))}
            {res.unknownSpace && <span className="text-[11px] text-amber">No space with that name</span>}
          </div>
        )}
        <div ref={listRef} className="min-h-0 flex-1 overflow-auto py-1.5">
          {shownCommands.length > 0 && (
            <Group label={commandMode || !q.trim() ? "Commands" : "Commands matching"}>
              {shownCommands.map((c, i) => (
                <Row key={c.id} idx={i} active={active === i} onHover={setActive} onClick={() => runIndex(i, "open")}>
                  <c.icon size={15} strokeWidth={1.7} className="text-ink-3" />
                  <span className="flex-1 truncate">{c.label}</span>
                  {c.hint && <span className="text-xs text-ink-3">{c.hint}</span>}
                </Row>
              ))}
            </Group>
          )}
          {!q.trim() && (
            <Group label="Filters">
              {FILTER_HINTS.map(([f, d]) => (
                <button
                  key={f}
                  type="button"
                  onClick={() => {
                    setQ((v) => (v ? v + " " : "") + f.split(":")[0] + ":");
                    inputRef.current?.focus();
                  }}
                  className="flex w-full items-center gap-3 px-4 py-1.5 text-left text-[13px] hover:bg-surface-2"
                >
                  <code className="rounded bg-surface-2 px-1.5 py-0.5 font-mono text-[11.5px] text-ink">{f}</code>
                  <span className="truncate text-xs text-ink-3">{d}</span>
                </button>
              ))}
            </Group>
          )}
          {files.length > 0 && (
            <Group label="Files">
              {files.map((f, j) => {
                const i = shownCommands.length + j;
                return (
                  <Row key={f.path} idx={i} active={active === i} onHover={setActive} onClick={() => runIndex(i, "open")}>
                    <FileIcon kind={f.kind} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate">{f.name}</div>
                      <div className="truncate text-[11.5px] text-ink-3">{shortPath(f.parent, home)}</div>
                    </div>
                    <span className="tabular shrink-0 text-right text-xs text-ink-3">
                      {f.isDir ? "" : formatBytes(f.size)}
                      <br />
                      {relativeTime(f.modified)}
                    </span>
                  </Row>
                );
              })}
            </Group>
          )}
          {res && files.length === 0 && !loading && (
            <p className="px-4 py-6 text-center text-[13px] text-ink-3">No files match. Try fewer words or different filters.</p>
          )}
        </div>
        <footer className="flex items-center gap-4 border-t border-line px-4 py-2 text-[11px] text-ink-3">
          <span className="flex items-center gap-1">
            <CornerDownLeft size={11} /> open
          </span>
          <span>Shift+Enter quick look</span>
          <span>Ctrl+Enter reveal</span>
          <span className="ml-auto">Esc to close</span>
        </footer>
      </div>
    </div>
  );
}

function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="mb-1">
      <div className="px-4 pb-1 pt-2 text-[11px] font-medium text-ink-3">{label}</div>
      {children}
    </div>
  );
}

function Row({
  idx,
  active,
  onHover,
  onClick,
  children,
}: {
  idx: number;
  active: boolean;
  onHover: (i: number) => void;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <div
      data-idx={idx}
      onMouseMove={() => !active && onHover(idx)}
      onClick={onClick}
      className={`mx-1.5 flex items-center gap-3 rounded-lg px-2.5 py-1.5 text-[13px] ${active ? "bg-accent-soft" : ""}`}
    >
      {children}
    </div>
  );
}
