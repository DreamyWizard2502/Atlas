import { useVirtualizer } from "@tanstack/react-virtual";
import { Clock } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { FileIcon } from "../components/FileIcon";
import { fileHandlers } from "../components/FileViews";
import { Empty, Pill, Select, Spinner } from "../components/ui";
import { api, type Space, type TimelineEntry } from "../lib/api";
import { toRef } from "../lib/actions";
import { dayKey, dayLabel, formatBytes, formatTime, parentName } from "../lib/format";
import { useStore, type FileRef } from "../lib/store";

const PAGE = 300;
const TYPES = ["image", "video", "audio", "pdf", "doc", "sheet", "slides", "text", "code", "archive", "cad", "installer"];

type Row = { type: "day"; key: string; count: number } | { type: "entry"; e: TimelineEntry };

export function Timeline() {
  const dataVersion = useStore((s) => s.dataVersion);
  const [kind, setKind] = useState("");
  const [spaceId, setSpaceId] = useState<number | null>(null);
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [entries, setEntries] = useState<TimelineEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [exhausted, setExhausted] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);
  const selected = useStore((s) => s.selection?.path);
  const reqId = useRef(0);

  useEffect(() => {
    api.listSpaces().then(setSpaces).catch(() => {});
  }, []);

  const load = async (reset: boolean) => {
    const id = ++reqId.current;
    setLoading(true);
    const before = reset ? null : entries[entries.length - 1]?.ts ?? null;
    try {
      const page = await api.timeline(before, PAGE, { kind: kind || null, spaceId });
      if (id !== reqId.current) return;
      setEntries((prev) => (reset ? page : [...prev, ...page]));
      setExhausted(page.length < PAGE);
    } finally {
      if (id === reqId.current) setLoading(false);
    }
  };

  useEffect(() => {
    load(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind, spaceId, dataVersion]);

  const rows: Row[] = useMemo(() => {
    const out: Row[] = [];
    const counts = new Map<string, number>();
    for (const e of entries) counts.set(dayKey(e.ts), (counts.get(dayKey(e.ts)) ?? 0) + 1);
    let last = "";
    for (const e of entries) {
      const k = dayKey(e.ts);
      if (k !== last) {
        out.push({ type: "day", key: k, count: counts.get(k) ?? 0 });
        last = k;
      }
      out.push({ type: "entry", e });
    }
    return out;
  }, [entries]);

  const refs: FileRef[] = useMemo(() => entries.filter((e) => e.exists).map((e) => toRef(e)), [entries]);

  const v = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (i) => (rows[i].type === "day" ? 48 : 40),
    overscan: 16,
  });

  const items = v.getVirtualItems();
  const lastItem = items[items.length - 1];
  useEffect(() => {
    if (lastItem && lastItem.index >= rows.length - 30 && !loading && !exhausted && entries.length) load(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lastItem?.index, rows.length, loading, exhausted]);

  // Sticky day header for the current scroll position.
  const first = items[0];
  let stickyDay: string | null = null;
  if (first) {
    for (let i = first.index; i >= 0; i--) {
      const r = rows[i];
      if (r?.type === "day") {
        stickyDay = r.key;
        break;
      }
    }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line px-6 pb-4 pt-5">
        <div className="flex-1">
          <h1 className="text-lg font-semibold tracking-tight">Timeline</h1>
          <p className="text-xs text-ink-3">Every change in your indexed locations, newest first</p>
        </div>
        <Select value={kind} onChange={(e) => setKind(e.target.value)} aria-label="File type">
          <option value="">All types</option>
          {TYPES.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </Select>
        <Select value={spaceId ?? ""} onChange={(e) => setSpaceId(e.target.value ? Number(e.target.value) : null)} aria-label="Space">
          <option value="">All spaces</option>
          {spaces.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </Select>
      </header>
      <div className="relative min-h-0 flex-1">
        {stickyDay && rows.length > 0 && (
          <div className="pointer-events-none absolute left-0 right-3 top-0 z-10 border-b border-line bg-bg/95 px-6 py-2 text-[13px] font-semibold backdrop-blur">
            {dayLabel(stickyDay)}
          </div>
        )}
        {entries.length === 0 && !loading ? (
          <Empty icon={<Clock size={22} />} title="No changes to show" body="Changes appear here as files are created, edited or deleted." />
        ) : (
          <div ref={parentRef} className="h-full overflow-auto">
            <div className="relative mx-auto max-w-[980px]" style={{ height: v.getTotalSize() }}>
              {items.map((it) => {
                const r = rows[it.index];
                if (r.type === "day") {
                  return (
                    <div key={`d-${r.key}`} className="absolute left-0 right-0 flex items-end gap-2 px-6 pb-2" style={{ top: it.start, height: it.size }}>
                      <span className="text-[13px] font-semibold">{dayLabel(r.key)}</span>
                      <span className="text-xs text-ink-3">{r.count >= PAGE ? `${r.count}+` : r.count} changes</span>
                    </div>
                  );
                }
                const e = r.e;
                const ref = toRef(e);
                const handlers = e.exists ? fileHandlers(ref, refs) : {};
                return (
                  <div key={`${e.path}-${e.ts}-${it.index}`} className="absolute left-0 right-0 px-6" style={{ top: it.start, height: it.size }}>
                    <div
                      {...handlers}
                      className={`relative ml-[54px] flex h-full items-center gap-3 rounded-lg pl-6 pr-3 ${
                        selected === e.path ? "bg-accent-soft" : e.exists ? "hover:bg-surface-2" : ""
                      }`}
                    >
                      <span className="tabular absolute -left-[54px] w-[44px] text-right text-xs text-ink-3">{formatTime(e.ts)}</span>
                      <span className="absolute -left-px bottom-0 top-0 w-px bg-line" />
                      <span
                        className={`absolute -left-[4px] h-2 w-2 rounded-full ring-2 ring-[var(--bg)] ${
                          e.change === "deleted" ? "bg-cold" : e.change === "created" ? "bg-accent" : "bg-line-strong"
                        }`}
                      />
                      <FileIcon kind={e.kind} />
                      <span className={`truncate text-[13px] ${e.exists ? "" : "text-ink-3 line-through"}`}>{e.name}</span>
                      <Pill tone={e.change === "created" ? "accent" : e.change === "deleted" ? "cold" : "neutral"}>{e.change}</Pill>
                      <span className="ml-auto truncate pl-3 text-xs text-ink-3" title={e.path}>
                        {parentName(e.path)}
                      </span>
                      <span className="tabular w-16 shrink-0 text-right text-xs text-ink-2">{e.change === "deleted" ? "" : formatBytes(e.size)}</span>
                    </div>
                  </div>
                );
              })}
            </div>
            {loading && (
              <div className="flex justify-center py-4">
                <Spinner />
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
