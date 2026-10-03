import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Archive,
  ArrowRight,
  Check,
  ChevronRight,
  Command,
  HardDrive,
  Inbox as InboxIcon,
  Layers,
  LayoutGrid,
  List,
  Search,
  Sparkles,
  TrendingUp,
} from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { GrowthChart, ReclaimBars, StorageList, Treemap, TreemapLegend } from "../components/Charts";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { FileCard, fileHandlers } from "../components/FileViews";
import { FileIcon } from "../components/FileIcon";
import { Button, Card, Checkbox, Empty, Modal, Pill, Segmented, Spinner } from "../components/ui";
import { api, type InboxItem, type Metrics, type TreeNode } from "../lib/api";
import { errorMessage, moveToHolding, toRef } from "../lib/actions";
import { formatBytes, formatCount, plural, relativeTime, sessionLabel, shortPath } from "../lib/format";
import { useLoad } from "../lib/hooks";
import { bumpData, navigate, setState, toast, useStore, type FileRef } from "../lib/store";

export function Dashboard() {
  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto flex max-w-[1480px] flex-col gap-4 px-6 pb-8 pt-5">
        <SearchBar />
        <Safe>
          <MetricCards />
        </Safe>
        <Safe>
          <JumpBackIn />
        </Safe>
        <div className="grid grid-cols-12 gap-4">
          <Safe span="xl:col-span-8">
            <StorageCard />
          </Safe>
          <Safe span="xl:col-span-4">
            <GrowthCard />
          </Safe>
        </div>
        <div className="grid grid-cols-12 gap-4">
          <Safe span="xl:col-span-7">
            <InboxCard />
          </Safe>
          <Safe span="xl:col-span-5">
            <CleanupCard />
          </Safe>
        </div>
        <Safe>
          <PinnedSpaces />
        </Safe>
      </div>
    </div>
  );
}

/** One failing widget shows its own small error instead of blanking the dashboard. */
function Safe({ children, span }: { children: React.ReactNode; span?: string }) {
  return (
    <ErrorBoundary
      fallback={(e, retry) => (
        <div className={`card col-span-12 flex items-center gap-3 px-4 py-3 text-xs text-ink-2 ${span ?? ""}`}>
          <span className="flex-1">This card couldn't load ({e.message}).</span>
          <Button size="sm" onClick={retry}>
            Try again
          </Button>
        </div>
      )}
    >
      {children}
    </ErrorBoundary>
  );
}

function SearchBar() {
  return (
    <button
      type="button"
      onClick={() => setState({ palette: true })}
      className="card flex h-11 w-full items-center gap-3 px-4 text-left text-ink-3 transition-colors duration-100 hover:border-line-strong"
    >
      <Search size={16} />
      <span className="flex-1 text-[13.5px]">Search everything — try “type:pdf modified:this week” or “size:&gt;500mb”</span>
      <kbd className="flex items-center gap-0.5 rounded-md border border-line px-1.5 py-0.5 text-[11px] text-ink-3">
        <Command size={11} />K
      </kbd>
    </button>
  );
}

// ---------------- Metric cards ----------------

function MetricCards() {
  const { data: m } = useLoad(api.metrics);
  return (
    <div className="grid grid-cols-2 gap-4 xl:grid-cols-4">
      <DriveTile m={m} />
      <Tile
        label="Growth this month"
        value={m?.growthMonth == null ? "—" : `${m.growthMonth >= 0 ? "+" : ""}${formatBytes(m.growthMonth)}`}
        sub={m?.growthSince ? `Since ${m.growthSince}` : "Collecting history"}
        icon={<TrendingUp size={15} />}
      />
      <Tile
        label="Inbox"
        value={m ? formatCount(m.inboxCount) : "—"}
        sub={m ? (m.inboxCount ? `${formatBytes(m.inboxBytes)} waiting in Downloads and Desktop` : "Downloads and Desktop are tidy") : ""}
        icon={<InboxIcon size={15} />}
        tone={m && m.inboxCount > 0 ? "amber" : undefined}
      />
      <Tile
        label="Reclaimable"
        value={m?.reclaimable == null ? "—" : formatBytes(m.reclaimable)}
        sub={m?.reclaimableAt ? `Analyzed ${relativeTime(m.reclaimableAt)}` : "Not analyzed yet"}
        icon={<Sparkles size={15} />}
        tone={m?.reclaimable ? "green" : undefined}
        onClick={() => navigate("cleanup")}
      />
    </div>
  );
}

function Tile({
  label,
  value,
  sub,
  icon,
  tone,
  onClick,
  children,
}: {
  label: string;
  value: string;
  sub?: string;
  icon: React.ReactNode;
  tone?: "amber" | "green";
  onClick?: () => void;
  children?: React.ReactNode;
}) {
  const valueTone = tone === "amber" ? "text-amber" : tone === "green" ? "text-green" : "text-ink";
  return (
    <div
      onClick={onClick}
      className={`card flex flex-col gap-1 px-4 py-3.5 ${onClick ? "transition-colors duration-100 hover:border-line-strong" : ""}`}
    >
      <div className="flex items-center justify-between text-xs text-ink-2">
        <span>{label}</span>
        <span className="text-ink-3">{icon}</span>
      </div>
      <div className={`text-[26px] font-semibold leading-tight tracking-tight ${valueTone}`}>{value}</div>
      {children}
      {sub && <div className="truncate text-xs text-ink-3">{sub}</div>}
    </div>
  );
}

function DriveTile({ m }: { m: Metrics | null }) {
  const d = m?.drive;
  const used = d ? d.total - d.free : 0;
  const pct = d && d.total ? used / d.total : 0;
  return (
    <Tile
      label={d ? `Drive ${d.mount.replace(/\\$/, "")}` : "Drive"}
      value={d ? formatBytes(used) : "—"}
      sub={d ? `of ${formatBytes(d.total)} · ${formatBytes(d.free)} free` : ""}
      icon={<HardDrive size={15} />}
      tone={pct > 0.9 ? "amber" : undefined}
    >
      <div className="my-1 h-1.5 rounded-full bg-accent-soft">
        <div
          className={`h-1.5 rounded-full ${pct > 0.9 ? "bg-amber-mark" : "bg-accent"}`}
          style={{ width: `${Math.round(pct * 100)}%` }}
          title={`${Math.round(pct * 100)}% used`}
        />
      </div>
    </Tile>
  );
}

// ---------------- Jump back in ----------------

function JumpBackIn() {
  const { data: sessions, loading } = useLoad(() => api.jumpBackIn(8));
  const selected = useStore((s) => s.selection?.path);
  const all: FileRef[] = useMemo(() => (sessions ?? []).flatMap((s) => s.files.map(toRef)), [sessions]);
  return (
    <Card title="Jump back in" subtitle="Files you opened or changed recently, grouped into work sessions">
      {loading && !sessions ? (
        <div className="flex h-[200px] items-center justify-center">
          <Spinner />
        </div>
      ) : !sessions?.length ? (
        <Empty title="Nothing recent yet" body="Files you open or modify will show up here." />
      ) : (
        <div className="flex flex-wrap gap-x-6 gap-y-4">
          {sessions.map((s) => (
            <div key={s.start} className="min-w-0">
              <div className="mb-2 text-xs text-ink-3">
                {sessionLabel(s.start, s.end)} · {plural(s.files.length, "file")}
              </div>
              <div className="flex flex-wrap gap-3">
                {s.files.map((f) => (
                  <div key={f.path} className="w-[160px]">
                    <FileCard
                      f={toRef(f)}
                      list={all}
                      selected={selected === f.path}
                      meta={
                        <>
                          {f.opened ? "Opened" : "Edited"} {relativeTime(f.lastTouched)}
                        </>
                      }
                    />
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}

// ---------------- Storage treemap ----------------

function StorageCard() {
  const [path, setPath] = useState<string | null>(null);
  const [view, setView] = useState<"map" | "list">("map");
  const [group, setGroup] = useState<{ label: string; items: TreeNode[] } | null>(null);
  const { data: tm, loading, error } = useLoad(() => api.treemap(path), [path]);
  const zoom = (p: string) => {
    setGroup(null);
    setPath(p);
  };
  return (
    <Card
      className="col-span-12 xl:col-span-8"
      title="Storage"
      subtitle={
        tm && tm.crumbs.length > 0 ? (
          <span className="flex min-w-0 flex-wrap items-center gap-0.5">
            {tm.crumbs.map(([label, p], i) => (
              <span key={p + i} className="flex items-center gap-0.5">
                {i > 0 && <ChevronRight size={11} />}
                <button
                  type="button"
                  className={`truncate hover:text-accent ${i === tm.crumbs.length - 1 ? "text-ink-2" : ""}`}
                  onClick={() => setPath(p || null)}
                >
                  {label}
                </button>
              </span>
            ))}
            <span className="ml-1.5 text-ink-3">· {formatBytes(tm.size)}</span>
          </span>
        ) : (
          "Click a folder to zoom in"
        )
      }
      action={
        <Segmented
          value={view}
          onChange={setView}
          options={[
            { value: "map", label: <LayoutGrid size={13} />, title: "Treemap" },
            { value: "list", label: <List size={13} />, title: "Sorted list" },
          ]}
        />
      }
    >
      <div className="flex h-[330px] flex-col gap-2">
        <div className="min-h-0 flex-1">
          {error ? (
            <Empty title="Couldn't load storage" body={error} />
          ) : tm && tm.children.length > 0 ? (
            view === "map" ? (
              <Treemap nodes={tm.children} total={tm.size} onZoom={zoom} onOpenGroup={(items, label) => setGroup({ items, label })} />
            ) : (
              <StorageList nodes={tm.children} total={tm.size} onZoom={zoom} />
            )
          ) : loading ? (
            <div className="flex h-full items-center justify-center">
              <Spinner />
            </div>
          ) : (
            <Empty title="No data yet" body="The treemap appears once indexing finishes." />
          )}
        </div>
        {tm && tm.children.length > 0 && <TreemapLegend nodes={tm.children} />}
      </div>
      {group && tm && (
        <Modal title={`${group.label} · ${formatBytes(group.items.reduce((a, n) => a + n.size, 0))}`} onClose={() => setGroup(null)} width={620}>
          <div className="h-[420px]">
            <StorageList nodes={group.items} total={tm.size} onZoom={zoom} />
          </div>
        </Modal>
      )}
    </Card>
  );
}

// ---------------- Growth ----------------

function GrowthCard() {
  const { data: g } = useLoad(() => api.growth(90));
  const points = g?.points ?? [];
  return (
    <Card
      className="col-span-12 xl:col-span-4"
      title="Drive growth"
      subtitle={g?.estimated ? "Indexed size, last 90 days · dashed part estimated from file dates" : "Indexed size, last 90 days"}
    >
      <div className="flex h-[330px] flex-col gap-3">
        <div className="min-h-0 flex-1">
          {points.length >= 2 ? (
            <GrowthChart points={points} />
          ) : (
            <Empty
              icon={<TrendingUp size={20} />}
              title="Collecting history"
              body={
                points.length === 1
                  ? `Atlas takes a snapshot every day. Today: ${formatBytes(points[0].size)}. The trend line appears tomorrow.`
                  : "Atlas takes a snapshot every day once indexing finishes."
              }
            />
          )}
        </div>
        {g?.fastest && (
          <button
            type="button"
            onClick={() => navigate("folders", { folderPath: g.fastest!.path })}
            className="flex items-center gap-3 rounded-lg border border-line px-3 py-2.5 text-left hover:bg-surface-2"
          >
            <TrendingUp size={16} className="shrink-0 text-amber" />
            <div className="min-w-0 flex-1">
              <div className="text-[11.5px] text-ink-3">Fastest growing folder</div>
              <div className="truncate text-[13px] font-medium">{g.fastest.name}</div>
            </div>
            <div className="tabular shrink-0 text-right text-xs">
              <div className="font-medium text-amber">+{formatBytes(g.fastest.delta)}</div>
              <div className="text-ink-3">since {g.fastest.since}</div>
            </div>
          </button>
        )}
      </div>
    </Card>
  );
}

// ---------------- Inbox ----------------

function InboxCard() {
  const home = useStore((s) => s.status?.home);
  const { data: inbox, reload } = useLoad(() => api.inbox(500));
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const items = inbox?.items ?? [];
  const parentRef = useRef<HTMLDivElement>(null);
  const v = useVirtualizer({ count: items.length, getScrollElement: () => parentRef.current, estimateSize: () => 44, overscan: 8 });
  const refs = useMemo(() => items.map(toRef), [items]);
  const sel = items.filter((i) => checked.has(i.path));
  const toggle = (p: string, on: boolean) =>
    setChecked((s) => {
      const n = new Set(s);
      if (on) n.add(p);
      else n.delete(p);
      return n;
    });
  const done = () => {
    setChecked(new Set());
    reload();
    bumpData();
  };
  const archive = async () => {
    try {
      const out = await api.archiveFiles(sel.map((s) => s.path));
      const ok = out.filter((o) => o.ok).length;
      toast(`Archived ${plural(ok, "file")}`, { tone: "success" });
      const bad = out.find((o) => !o.ok);
      if (bad) toast(`Skipped: ${bad.detail}`, { tone: "attention" });
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
    }
    done();
  };
  const keep = async () => {
    await api.inboxDismiss(sel.map((s) => s.path));
    toast(`Kept ${plural(sel.length, "file")}; they won't show in the inbox again`);
    done();
  };

  return (
    <Card
      className="col-span-12 xl:col-span-7"
      title="Inbox"
      subtitle={
        inbox
          ? `${plural(inbox.total, "item")} in Downloads and Desktop older than ${inbox.thresholdDays} days · ${formatBytes(inbox.totalSize)}`
          : "Downloads and Desktop"
      }
      bodyClass="flex flex-col"
      action={
        items.length > 0 && (
          <Checkbox
            label="Select all"
            checked={sel.length === items.length}
            indeterminate={sel.length > 0 && sel.length < items.length}
            onChange={(on) => setChecked(on ? new Set(items.map((i) => i.path)) : new Set())}
          />
        )
      }
    >
      {items.length === 0 ? (
        <div className="h-[300px]">
          <Empty icon={<Check size={20} />} title="Inbox zero" body="Nothing old is lingering in Downloads or on the Desktop." />
        </div>
      ) : (
        <>
          <div ref={parentRef} className="mt-2 h-[300px] overflow-auto">
            <div style={{ height: v.getTotalSize(), position: "relative" }}>
              {v.getVirtualItems().map((row) => {
                const it = items[row.index];
                return (
                  <InboxRow
                    key={it.path}
                    it={it}
                    list={refs}
                    home={home}
                    checked={checked.has(it.path)}
                    onCheck={(on) => toggle(it.path, on)}
                    style={{ top: row.start, height: row.size }}
                  />
                );
              })}
            </div>
          </div>
          <div className="flex min-h-12 items-center gap-2 border-t border-line px-4 py-2">
            {sel.length > 0 ? (
              <>
                <span className="mr-auto text-xs text-ink-2">
                  {plural(sel.length, "item")} · {formatBytes(sel.reduce((a, b) => a + b.size, 0))}
                </span>
                <Button size="sm" onClick={() => {
                    setState({ dialog: { kind: "space", files: sel.map(toRef), fromInbox: true } });
                    setChecked(new Set());
                  }}>
                  <Layers size={13} /> Move to space
                </Button>
                <Button size="sm" onClick={archive} title="Move into your archive folder">
                  <Archive size={13} /> Archive
                </Button>
                <Button size="sm" onClick={keep} title="Keep where it is and stop flagging it">
                  Keep
                </Button>
                <Button
                  size="sm"
                  variant="success"
                  onClick={async () => {
                    await moveToHolding(sel.map(toRef));
                    done();
                  }}
                  title="Move to the holding area (restorable)"
                >
                  Clean
                </Button>
              </>
            ) : (
              <span className="text-xs text-ink-3">Select items to add them to a space, archive, keep or clean. Cleaning is always restorable.</span>
            )}
          </div>
        </>
      )}
    </Card>
  );
}

function InboxRow({
  it,
  list,
  home,
  checked,
  onCheck,
  style,
}: {
  it: InboxItem;
  list: FileRef[];
  home?: string | null;
  checked: boolean;
  onCheck: (v: boolean) => void;
  style: React.CSSProperties;
}) {
  const selected = useStore((s) => s.selection?.path === it.path);
  return (
    <div
      {...fileHandlers(toRef(it), list)}
      className={`absolute left-0 right-0 flex items-center gap-3 px-4 ${selected ? "bg-accent-soft" : "hover:bg-surface-2"}`}
      style={style}
    >
      <Checkbox checked={checked} onChange={onCheck} label={`Select ${it.name}`} />
      <FileIcon kind={it.kind} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[13px]">{it.name}</span>
          {it.installedAs && (
            <Pill tone="amber" title={`Looks like ${it.installedAs} is already installed`}>
              Already installed
            </Pill>
          )}
          {it.duplicateOf && (
            <Pill tone="amber" title={`Same name as ${it.duplicateOf}`}>
              Likely duplicate
            </Pill>
          )}
        </div>
        <div className="truncate text-[11.5px] text-ink-3">
          {it.source} · {shortPath(it.parent, home)}
        </div>
      </div>
      <span className="tabular w-16 shrink-0 text-right text-xs text-ink-3">{it.ageDays} days</span>
      <span className="tabular w-16 shrink-0 text-right text-xs text-ink-2">{it.isDir ? "" : formatBytes(it.size)}</span>
    </div>
  );
}

// ---------------- Cleanup summary ----------------

function CleanupCard() {
  const { data: st } = useLoad(api.cleanupSummary);
  const analyzing = st?.analyzing;
  const s = st?.summary;
  return (
    <Card
      className="col-span-12 xl:col-span-5"
      title="Cleanup"
      subtitle={s ? `Analyzed ${relativeTime(s.analyzedAt)}` : "Find duplicates, cold files, caches and junk"}
      action={
        <Button size="sm" variant="ghost" onClick={() => navigate("cleanup")}>
          Review <ArrowRight size={13} />
        </Button>
      }
    >
      <div className="flex h-[348px] flex-col">
        {s ? (
          <>
            <div className="mb-3">
              <div className="text-[28px] font-semibold leading-tight tracking-tight text-green">{formatBytes(s.totalReclaimable)}</div>
              <div className="text-xs text-ink-3">could be reclaimed. Nothing is removed without your review.</div>
            </div>
            <div className="min-h-0 flex-1 overflow-auto">
              <ReclaimBars
                rows={s.categories
                  .filter((c) => c.group !== "review")
                  .map((c) => ({ id: c.id, label: c.label, value: c.reclaimable, count: c.count }))}
                onClick={() => navigate("cleanup")}
              />
            </div>
            {s.reviewTotal > 0 && (
              <button
                type="button"
                onClick={() => navigate("cleanup")}
                className="mt-2 flex items-center justify-between rounded-lg border border-line px-3 py-2 text-left text-xs hover:bg-surface-2"
              >
                <span className="text-ink-2">Review or archive: large files untouched for months</span>
                <span className="tabular font-medium text-cold">{formatBytes(s.reviewTotal)}</span>
              </button>
            )}
          </>
        ) : (
          <Empty
            icon={<Sparkles size={20} />}
            title={analyzing ? "Analyzing…" : "No analysis yet"}
            body="Atlas looks for duplicates, large cold files, temp files and app junk. Analysis never changes anything."
            action={
              !analyzing && (
                <Button variant="primary" onClick={() => api.cleanupAnalyze().then(() => toast("Analysis started"))}>
                  Analyze now
                </Button>
              )
            }
          />
        )}
      </div>
    </Card>
  );
}

// ---------------- Pinned spaces ----------------

function PinnedSpaces() {
  const spacesVersion = useStore((s) => s.spacesVersion);
  const { data: spaces } = useLoad(api.listSpaces, [spacesVersion]);
  const pinned = (spaces ?? []).filter((s) => s.pinned);
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="mr-1 text-xs text-ink-3">Pinned spaces</span>
      {pinned.map((s) => (
        <button
          key={s.id}
          type="button"
          onClick={() => navigate("spaces", { spaceId: s.id })}
          className="flex h-8 items-center gap-2 rounded-full border border-line bg-surface px-3 text-[13px] transition-colors duration-100 hover:border-accent"
        >
          <Layers size={13} className="text-accent" />
          {s.name}
          <span className="tabular text-xs text-ink-3">{formatCount(s.count)}</span>
        </button>
      ))}
      <button
        type="button"
        onClick={() => navigate("spaces")}
        className="flex h-8 items-center gap-1.5 rounded-full border border-dashed border-line-strong px-3 text-[13px] text-ink-3 hover:border-accent hover:text-accent"
      >
        {pinned.length ? "All spaces" : "Create a space"}
        <ArrowRight size={13} />
      </button>
    </div>
  );
}
