import { useVirtualizer } from "@tanstack/react-virtual";
import { Archive, FlaskConical, RotateCcw, ScrollText, ShieldCheck, Sparkles } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { FileIcon } from "../components/FileIcon";
import { fileHandlers } from "../components/FileViews";
import { Button, Checkbox, Empty, Modal, Pill, Segmented, Spinner } from "../components/ui";
import { api, on, type AnalyzeProgress, type Candidate, type CleanupState, type HeldItem, type LogEntry, type RunReport } from "../lib/api";
import { errorMessage, restoreBatch } from "../lib/actions";
import { formatBytes, formatCount, formatDate, plural, relativeTime, shortPath } from "../lib/format";
import { useLoad } from "../lib/hooks";
import { bumpData, toast, useStore, type FileRef } from "../lib/store";

const DESCRIPTIONS: Record<string, string> = {
  duplicates: "Files with identical content. Nothing is selected — pick the copy to keep in each group.",
  large_cold: "Big files nobody has modified in a long time.",
  temp: "Files older than a day in temp folders and known browser/app caches.",
  junk: "CAD backups, autosaves, lock files and Office ~$ files, from your rule list in Settings.",
  installers: "Setup files in Downloads you probably don't need any more.",
  empty_folders: "Folders that contain no files at all.",
};

const CATEGORY_LABEL: Record<string, string> = {
  duplicates: "Duplicates",
  large_cold: "Large and cold",
  temp: "Temp and caches",
  junk: "App junk",
  installers: "Old installers",
  empty_folders: "Empty folders",
  manual: "Manual",
};

export function Cleanup() {
  const [tab, setTab] = useState<"review" | "holding" | "log">("review");
  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-4 border-b border-line px-6 pb-4 pt-5">
        <div className="flex-1">
          <h1 className="text-lg font-semibold tracking-tight">Cleanup</h1>
          <p className="flex items-center gap-1.5 text-xs text-ink-3">
            <ShieldCheck size={13} className="text-green" />
            Nothing is ever deleted. Cleaned items wait in a holding area, restorable with one click, then go to the Recycle Bin.
          </p>
        </div>
        <Segmented
          value={tab}
          onChange={setTab}
          options={[
            { value: "review", label: "Review" },
            { value: "holding", label: "Holding area" },
            { value: "log", label: "Log" },
          ]}
        />
      </header>
      <div className="min-h-0 flex-1">
        {tab === "review" && <Review />}
        {tab === "holding" && <Holding />}
        {tab === "log" && <Log />}
      </div>
    </div>
  );
}

// ---------------- Review ----------------

function Review() {
  const [state, setStateLocal] = useState<CleanupState | null>(null);
  const [progress, setProgress] = useState<AnalyzeProgress | null>(null);
  const [category, setCategory] = useState("duplicates");
  const dataVersion = useStore((s) => s.dataVersion);

  const refresh = () => api.cleanupSummary().then(setStateLocal);
  useEffect(() => {
    refresh();
  }, [dataVersion]);
  useEffect(() => {
    const subs = [
      on<AnalyzeProgress>("cleanup://progress", setProgress),
      on("cleanup://done", () => {
        setProgress(null);
        refresh();
      }),
      on<string>("cleanup://error", (e) => {
        setProgress(null);
        toast(`Analysis failed: ${e}`, { tone: "attention" });
        refresh();
      }),
    ];
    return () => subs.forEach((p) => p.then((u) => u()));
  }, []);

  const analyze = async () => {
    await api.cleanupAnalyze();
    setStateLocal((s) => (s ? { ...s, analyzing: true } : s));
  };

  const analyzing = state?.analyzing || progress != null;
  const s = state?.summary;

  if (!state) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }

  if (!state.hasItems) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="card max-w-md p-6 text-center">
          <Sparkles size={24} className="mx-auto mb-3 text-ink-3" />
          <h2 className="text-[15px] font-semibold">{analyzing ? "Analyzing your files…" : "Find space to reclaim"}</h2>
          <p className="mt-1 text-[13px] text-ink-2">
            {analyzing
              ? progressText(progress)
              : s
                ? `The last analysis (${relativeTime(s.analyzedAt)}) found ${formatBytes(s.totalReclaimable)}. Run it again to review the items.`
                : "Atlas looks for duplicates, large cold files, temp files, app junk, old installers and empty folders. Analysis is read-only."}
          </p>
          <div className="mt-4">
            {analyzing ? (
              <Button onClick={() => api.cleanupCancel()}>Cancel</Button>
            ) : (
              <Button variant="primary" onClick={analyze}>
                Analyze now
              </Button>
            )}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      <aside className="flex w-[280px] shrink-0 flex-col border-r border-line bg-surface">
        <div className="px-5 pb-3 pt-4">
          <div className="text-xs text-ink-3">Reclaimable</div>
          <div className="text-[28px] font-semibold leading-tight tracking-tight text-green">{formatBytes(s?.totalReclaimable ?? 0)}</div>
          <div className="mt-1 flex items-center gap-2 text-xs text-ink-3">
            {analyzing ? (
              <>
                <Spinner size={12} /> {progressText(progress)}
              </>
            ) : (
              <>
                Analyzed {relativeTime(s?.analyzedAt)}
                <button type="button" className="text-accent hover:underline" onClick={analyze}>
                  Re-run
                </button>
              </>
            )}
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-2 pb-3">
          {s?.categories.map((c) => (
            <button
              key={c.id}
              type="button"
              onClick={() => setCategory(c.id)}
              className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left transition-colors duration-100 ${
                category === c.id ? "bg-accent-soft" : "hover:bg-surface-2"
              }`}
            >
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-medium">{c.label}</div>
                <div className="text-[11.5px] text-ink-3">
                  {c.id === "duplicates" ? plural(c.count, "group") : plural(c.count, "item")}
                  {c.truncated && " (first 20,000)"}
                </div>
              </div>
              <span className={`tabular text-xs font-medium ${c.reclaimable > 0 ? "text-green" : "text-ink-3"}`}>{formatBytes(c.reclaimable)}</span>
            </button>
          ))}
        </div>
      </aside>
      <section className="min-w-0 flex-1">
        <CategoryReview key={`${category}-${s?.analyzedAt}`} category={category} onRan={refresh} />
      </section>
    </div>
  );
}

function progressText(p: AnalyzeProgress | null) {
  if (!p) return "Working…";
  if (p.total > 0) return `${p.phase}: ${formatCount(p.done)} of ${formatCount(p.total)}`;
  return `${p.phase}…`;
}

type Row = { type: "group"; id: number; size: number; count: number } | { type: "item"; c: Candidate };

function CategoryReview({ category, onRan }: { category: string; onRan: () => void }) {
  const home = useStore((s) => s.status?.home);
  const { data: items, loading, setData } = useLoad(() => api.cleanupItems(category), [category], { live: false });
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [keep, setKeep] = useState<Map<number, string>>(new Map());
  const [dryRun, setDryRun] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<RunReport | null>(null);
  const parentRef = useRef<HTMLDivElement>(null);
  const isDup = category === "duplicates";
  const list = items ?? [];

  const groups = useMemo(() => {
    const m = new Map<number, Candidate[]>();
    if (isDup) for (const c of list) m.set(c.group ?? 0, [...(m.get(c.group ?? 0) ?? []), c]);
    return m;
  }, [list, isDup]);

  const rows: Row[] = useMemo(() => {
    if (!isDup) return list.map((c) => ({ type: "item" as const, c }));
    const out: Row[] = [];
    for (const [id, members] of groups) {
      if (members.length < 2) continue;
      out.push({ type: "group", id, size: members[0].size, count: members.length });
      for (const c of members) out.push({ type: "item", c });
    }
    return out;
  }, [list, groups, isDup]);

  const refs: FileRef[] = useMemo(
    () => list.map((c) => ({ path: c.path, name: c.name, kind: c.isDir ? "folder" : kindFromName(c.name), isDir: c.isDir, size: c.size, ext: extOf(c.name) })),
    [list],
  );
  const refByPath = useMemo(() => new Map(refs.map((r) => [r.path, r])), [refs]);

  const v = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (i) => (rows[i].type === "group" ? 40 : 46),
    overscan: 12,
  });

  const selected = list.filter((c) => checked.has(c.path));
  const selectedBytes = selected.reduce((a, c) => a + c.size, 0);

  const toggle = (p: string, on: boolean) =>
    setChecked((s) => {
      const n = new Set(s);
      if (on) n.add(p);
      else n.delete(p);
      return n;
    });

  const chooseKeep = (groupId: number, path: string) => {
    setKeep((k) => new Map(k).set(groupId, path));
    const members = groups.get(groupId) ?? [];
    setChecked((s) => {
      const n = new Set(s);
      for (const m of members) {
        if (m.path === path) n.delete(m.path);
        else n.add(m.path);
      }
      return n;
    });
  };

  const keepNewestEverywhere = () => {
    const k = new Map<number, string>();
    const n = new Set<string>();
    for (const [id, members] of groups) {
      if (members.length < 2) continue;
      const newest = [...members].sort((a, b) => (b.modified ?? 0) - (a.modified ?? 0))[0];
      k.set(id, newest.path);
      for (const m of members) if (m.path !== newest.path) n.add(m.path);
    }
    setKeep(k);
    setChecked(n);
  };

  const run = async () => {
    setConfirm(false);
    setRunning(true);
    try {
      const r = await api.cleanupRun(
        selected.map((c) => ({ path: c.path, category: c.category, size: c.size })),
        dryRun,
      );
      setReport(r);
      if (!r.dryRun) {
        const moved = new Set(r.outcomes.filter((o) => o.ok).map((o) => o.path));
        setData((d) => (d ?? []).filter((c) => !moved.has(c.path)));
        setChecked(new Set());
        setKeep(new Map());
        onRan();
        bumpData();
      }
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
    } finally {
      setRunning(false);
    }
  };

  const allSelectable = list;
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 border-b border-line px-5 py-3">
        {!isDup && list.length > 0 && (
          <Checkbox
            label="Select all"
            checked={selected.length === allSelectable.length && allSelectable.length > 0}
            indeterminate={selected.length > 0 && selected.length < allSelectable.length}
            onChange={(on) => setChecked(on ? new Set(allSelectable.map((c) => c.path)) : new Set())}
          />
        )}
        <p className="flex-1 text-xs text-ink-2">{DESCRIPTIONS[category]}</p>
        {isDup && groups.size > 0 && (
          <Button size="sm" onClick={keepNewestEverywhere} title="Mark the most recently modified copy as the one to keep, in every group">
            Keep newest in every group
          </Button>
        )}
      </div>
      <div className="min-h-0 flex-1">
        {loading && !items ? (
          <div className="flex h-full items-center justify-center">
            <Spinner />
          </div>
        ) : rows.length === 0 ? (
          <Empty icon={<Sparkles size={20} />} title="Nothing here" body="This category has nothing to clean right now." />
        ) : (
          <div ref={parentRef} className="h-full overflow-auto">
            <div style={{ height: v.getTotalSize(), position: "relative" }}>
              {v.getVirtualItems().map((it) => {
                const r = rows[it.index];
                if (r.type === "group") {
                  const kept = keep.get(r.id);
                  return (
                    <div
                      key={`g-${r.id}`}
                      className="absolute left-0 right-0 flex items-end gap-2 border-t border-line px-5 pb-1.5 text-xs"
                      style={{ top: it.start, height: it.size }}
                    >
                      <span className="font-medium text-ink">{plural(r.count, "copy", "copies")}</span>
                      <span className="text-ink-3">of {formatBytes(r.size)}</span>
                      <span className="text-ink-3">·</span>
                      <span className="text-green">{formatBytes(r.size * (r.count - 1))} reclaimable</span>
                      {!kept && <span className="ml-auto text-amber">Choose a copy to keep</span>}
                    </div>
                  );
                }
                const c = r.c;
                const ref = refByPath.get(c.path)!;
                const isKept = isDup && keep.get(c.group ?? -1) === c.path;
                return (
                  <div
                    key={c.path}
                    {...fileHandlers(ref, refs)}
                    className={`absolute left-0 right-0 flex items-center gap-3 px-5 hover:bg-surface-2 ${isKept ? "bg-green-soft/60" : ""}`}
                    style={{ top: it.start, height: it.size }}
                  >
                    {isDup && (
                      <label className="flex shrink-0 items-center gap-1.5 text-xs text-ink-2" onClick={(e) => e.stopPropagation()}>
                        <input
                          type="radio"
                          name={`keep-${c.group}`}
                          checked={isKept}
                          onChange={() => chooseKeep(c.group ?? 0, c.path)}
                          className="accent-[var(--green-mark)]"
                        />
                        Keep
                      </label>
                    )}
                    <Checkbox checked={checked.has(c.path)} onChange={(on) => toggle(c.path, on)} label={`Clean ${c.name}`} />
                    <FileIcon kind={ref.kind} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="truncate text-[13px]">{c.name}</span>
                        {!isDup && <span className="truncate text-[11.5px] text-ink-3">{c.reason}</span>}
                      </div>
                      <div className="truncate text-[11.5px] text-ink-3" title={c.path}>
                        {shortPath(c.path, home)}
                      </div>
                    </div>
                    <span className="tabular w-28 shrink-0 text-right text-xs text-ink-3">{formatDate(c.modified, false)}</span>
                    <span className="tabular w-20 shrink-0 text-right text-xs text-ink-2">{c.isDir ? "" : formatBytes(c.size)}</span>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
      <footer className="flex items-center gap-3 border-t border-line bg-surface px-5 py-3">
        <span className="text-[13px]">
          {selected.length ? (
            <>
              <span className="font-medium">{plural(selected.length, "item")}</span>
              <span className="text-ink-3"> selected · </span>
              <span className="tabular font-medium text-green">{formatBytes(selectedBytes)}</span>
            </>
          ) : (
            <span className="text-ink-3">Select items to clean</span>
          )}
        </span>
        <label className="ml-auto flex items-center gap-2 text-xs text-ink-2" title="Validate everything and log what would happen, without moving anything">
          <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)} className="accent-[var(--accent)]" />
          <FlaskConical size={13} /> Dry run
        </label>
        <Button variant={dryRun ? "primary" : "success"} disabled={!selected.length || running} onClick={() => setConfirm(true)}>
          {running && <Spinner />}
          {dryRun ? "Run dry run" : "Move to holding area"}
        </Button>
      </footer>
      {confirm && (
        <Modal
          title={dryRun ? "Run a dry run?" : `Move ${plural(selected.length, "item")} to the holding area?`}
          onClose={() => setConfirm(false)}
          footer={
            <>
              <Button onClick={() => setConfirm(false)}>Cancel</Button>
              <Button variant={dryRun ? "primary" : "success"} onClick={run}>
                {dryRun ? "Run dry run" : `Move ${formatBytes(selectedBytes)}`}
              </Button>
            </>
          }
        >
          <p className="text-[13px] text-ink-2">
            {dryRun
              ? "Every item is checked exactly as in a real run, and the result is logged. Nothing is moved."
              : "Items are moved (not copied, not deleted) into the holding area on the same drive. You can restore them with one click. After the retention period they go to the Recycle Bin."}
          </p>
          <p className="mt-3 text-xs text-ink-3">
            Each item is re-checked first: files that changed, are open in another app, or sit in protected locations are skipped.
          </p>
        </Modal>
      )}
      {report && <ReportModal report={report} onClose={() => setReport(null)} />}
    </div>
  );
}

function ReportModal({ report, onClose }: { report: RunReport; onClose: () => void }) {
  const skipped = report.outcomes.filter((o) => !o.ok);
  return (
    <Modal
      title={report.dryRun ? "Dry run complete" : "Cleanup complete"}
      onClose={onClose}
      width={560}
      footer={
        <>
          {!report.dryRun && report.moved > 0 && (
            <Button
              onClick={async () => {
                await restoreBatch(report.batchId);
                onClose();
              }}
            >
              <RotateCcw size={13} /> Restore all
            </Button>
          )}
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </>
      }
    >
      <p className="text-[13px]">
        {report.dryRun ? "Would move " : "Moved "}
        <span className="font-medium">{plural(report.moved, "item")}</span> ({formatBytes(report.bytes)})
        {report.dryRun ? "." : " to the holding area."}
        {skipped.length > 0 && <> Skipped {plural(skipped.length, "item")}.</>}
      </p>
      {skipped.length > 0 && (
        <div className="mt-3 max-h-64 space-y-1 overflow-auto rounded-lg border border-line p-2">
          {skipped.slice(0, 200).map((o) => (
            <div key={o.path} className="text-xs">
              <div className="truncate text-ink">{o.path}</div>
              <div className="text-amber">{o.detail}</div>
            </div>
          ))}
        </div>
      )}
    </Modal>
  );
}

// ---------------- Holding area ----------------

function Holding() {
  const [history, setHistory] = useState(false);
  const { data, loading, reload } = useLoad(() => api.holdingList(history), [history]);
  const [checked, setChecked] = useState<Set<number>>(new Set());
  const parentRef = useRef<HTMLDivElement>(null);
  const items = data ?? [];
  const v = useVirtualizer({ count: items.length, getScrollElement: () => parentRef.current, estimateSize: () => 48, overscan: 10 });
  const held = items.filter((i) => i.status === "held");
  const sel = held.filter((i) => checked.has(i.id));
  const totalHeld = held.reduce((a, b) => a + b.size, 0);

  const restore = async (ids: number[]) => {
    try {
      const out = await api.holdingRestore(ids);
      const ok = out.filter((o) => o.ok);
      const renamed = ok.filter((o) => o.detail.startsWith("Restored as"));
      toast(`Restored ${plural(ok.length, "item")}${renamed.length ? ` (${renamed.length} renamed to avoid overwriting)` : ""}`, { tone: "success" });
      const bad = out.find((o) => !o.ok);
      if (bad) toast(`${bad.path}: ${bad.detail}`, { tone: "attention" });
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
    }
    setChecked(new Set());
    reload();
    bumpData();
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-3 border-b border-line px-6 py-3">
        <Archive size={15} className="text-ink-3" />
        <span className="text-[13px]">
          {plural(held.length, "item")} held · <span className="tabular">{formatBytes(totalHeld)}</span>
        </span>
        <label className="ml-4 flex items-center gap-2 text-xs text-ink-2">
          <input type="checkbox" checked={history} onChange={(e) => setHistory(e.target.checked)} className="accent-[var(--accent)]" />
          Show restored and recycled
        </label>
        <div className="ml-auto flex gap-2">
          <Button size="sm" disabled={!sel.length} onClick={() => restore(sel.map((s) => s.id))}>
            <RotateCcw size={13} /> Restore selected
          </Button>
          <Button size="sm" variant="ghost" disabled={!held.length} onClick={() => restore(held.map((s) => s.id))}>
            Restore all
          </Button>
        </div>
      </div>
      <div className="min-h-0 flex-1">
        {loading && !data ? (
          <div className="flex h-full items-center justify-center">
            <Spinner />
          </div>
        ) : items.length === 0 ? (
          <Empty icon={<Archive size={20} />} title="The holding area is empty" body="Cleaned items wait here before going to the Recycle Bin." />
        ) : (
          <div ref={parentRef} className="h-full overflow-auto">
            <div style={{ height: v.getTotalSize(), position: "relative" }}>
              {v.getVirtualItems().map((row) => {
                const h = items[row.index];
                return <HeldRow key={h.id} h={h} checked={checked.has(h.id)} onCheck={(on) => setChecked((s) => toggleSet(s, h.id, on))} onRestore={() => restore([h.id])} style={{ top: row.start, height: row.size }} />;
              })}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function toggleSet<T>(s: Set<T>, v: T, on: boolean) {
  const n = new Set(s);
  if (on) n.add(v);
  else n.delete(v);
  return n;
}

function HeldRow({ h, checked, onCheck, onRestore, style }: { h: HeldItem; checked: boolean; onCheck: (v: boolean) => void; onRestore: () => void; style: React.CSSProperties }) {
  const home = useStore((s) => s.status?.home);
  const name = h.originalPath.split(/[\\/]/).pop() ?? h.originalPath;
  const days = Math.max(0, Math.ceil((h.expiresAt - Date.now() / 1000) / 86400));
  return (
    <div className="absolute left-0 right-0 flex items-center gap-3 px-6 hover:bg-surface-2" style={style}>
      {h.status === "held" ? <Checkbox checked={checked} onChange={onCheck} label={`Select ${name}`} /> : <span className="w-3.5" />}
      <FileIcon kind={h.isDir ? "folder" : kindFromName(name)} />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px]">{name}</div>
        <div className="truncate text-[11.5px] text-ink-3" title={h.originalPath}>
          {shortPath(h.originalPath, home)}
        </div>
      </div>
      <Pill>{CATEGORY_LABEL[h.category] ?? h.category}</Pill>
      <span className="tabular w-32 shrink-0 text-right text-xs text-ink-3">{formatDate(h.heldAt)}</span>
      <span className="w-36 shrink-0 text-right text-xs">
        {h.status === "held" ? (
          <span className={days <= 3 ? "text-amber" : "text-ink-3"}>Recycle Bin in {plural(days, "day")}</span>
        ) : (
          <Pill tone={h.status === "restored" ? "green" : "cold"}>{h.status === "trashed" ? "In Recycle Bin" : h.status}</Pill>
        )}
      </span>
      <span className="tabular w-20 shrink-0 text-right text-xs text-ink-2">{h.isDir ? "" : formatBytes(h.size)}</span>
      <span className="w-20 shrink-0 text-right">
        {h.status === "held" && (
          <Button size="sm" variant="ghost" onClick={onRestore}>
            Restore
          </Button>
        )}
      </span>
    </div>
  );
}

// ---------------- Log ----------------

const ACTION_TONE: Record<string, "neutral" | "green" | "amber" | "accent" | "cold"> = {
  hold: "green",
  restore: "accent",
  would_hold: "neutral",
  skip: "amber",
  error: "amber",
  trash: "cold",
  archive: "accent",
};

function Log() {
  const { data, loading } = useLoad(() => api.cleanupLog(2000));
  const parentRef = useRef<HTMLDivElement>(null);
  const items: LogEntry[] = data ?? [];
  const v = useVirtualizer({ count: items.length, getScrollElement: () => parentRef.current, estimateSize: () => 44, overscan: 10 });
  if (loading && !data) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }
  if (!items.length) return <Empty icon={<ScrollText size={20} />} title="No cleanup actions yet" body="Every move, restore, skip and dry run is recorded here." />;
  return (
    <div ref={parentRef} className="h-full overflow-auto">
      <div style={{ height: v.getTotalSize(), position: "relative" }}>
        {v.getVirtualItems().map((row) => {
          const l = items[row.index];
          return (
            <div key={l.id} className="absolute left-0 right-0 flex items-center gap-3 px-6 text-xs" style={{ top: row.start, height: row.size }}>
              <span className="tabular w-36 shrink-0 text-ink-3">{formatDate(l.ts)}</span>
              <span className="w-24 shrink-0">
                <Pill tone={ACTION_TONE[l.action] ?? "neutral"}>{l.action.replace("_", " ")}</Pill>
              </span>
              <div className="min-w-0 flex-1">
                <div className="selectable truncate text-ink" title={l.path}>
                  {l.path}
                </div>
                {l.detail && <div className="truncate text-ink-3" title={l.detail}>{l.detail}</div>}
              </div>
              {l.dryRun && <Pill>dry run</Pill>}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---------------- helpers ----------------

function extOf(name: string) {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

const EXT_KIND: Record<string, FileRef["kind"]> = {};
for (const [k, exts] of Object.entries({
  image: "jpg jpeg png gif webp bmp tif tiff heic svg psd",
  video: "mp4 mov mkv avi wmv webm m4v",
  audio: "mp3 wav flac aac ogg m4a",
  pdf: "pdf",
  doc: "doc docx odt rtf",
  sheet: "xls xlsx csv ods",
  slides: "ppt pptx odp",
  archive: "zip rar 7z tar gz iso",
  cad: "sldprt sldasm slddrw dwg dxf step stp stl ipt iam 3dm skp f3d",
  installer: "exe msi msix appx",
  code: "js ts py rs json xml html css",
  text: "txt md log",
}))
  for (const e of exts.split(" ")) EXT_KIND[e] = k as FileRef["kind"];

function kindFromName(name: string): FileRef["kind"] {
  return EXT_KIND[extOf(name)] ?? "other";
}
