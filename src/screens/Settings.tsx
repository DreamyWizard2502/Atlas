import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, FolderPlus, Plus, RefreshCw, RotateCcw, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Button, Card, IconButton, Spinner, TextInput } from "../components/ui";
import { api, type JunkRule, type Settings } from "../lib/api";
import { errorMessage } from "../lib/actions";
import { formatCount, relativeTime } from "../lib/format";
import { toast, useStore } from "../lib/store";

export function SettingsScreen() {
  const status = useStore((s) => s.status);
  const [saved, setSaved] = useState<Settings | null>(null);
  const [s, setS] = useState<Settings | null>(null);
  const [newExclusion, setNewExclusion] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.getSettings().then((x) => {
      setSaved(x);
      setS(x);
    });
  }, []);

  if (!s || !saved) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }

  const dirty = JSON.stringify(s) !== JSON.stringify(saved);
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS({ ...s, [k]: v });
  const setRule = (i: number, patch: Partial<JunkRule>) => set("junkRules", s.junkRules.map((r, j) => (j === i ? { ...r, ...patch } : r)));

  const pickFolders = async (): Promise<string[]> => {
    const picked = await open({ directory: true, multiple: true });
    return Array.isArray(picked) ? picked : picked ? [picked] : [];
  };

  const save = async () => {
    setBusy(true);
    try {
      const out = await api.saveSettings(s);
      const reindex = JSON.stringify(out.roots) !== JSON.stringify(saved.roots) || JSON.stringify(out.excluded) !== JSON.stringify(saved.excluded);
      setSaved(out);
      setS(out);
      toast(reindex ? "Settings saved. Re-indexing with the new locations…" : "Settings saved", { tone: "success" });
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line px-6 pb-4 pt-5">
        <div className="flex-1">
          <h1 className="text-lg font-semibold tracking-tight">Settings</h1>
          <p className="text-xs text-ink-3">
            {status ? `${formatCount(status.indexedFiles)} files indexed` : ""}
            {status?.lastScanAt ? ` · last full scan ${relativeTime(status.lastScanAt)}` : ""}
          </p>
        </div>
        <Button onClick={() => api.startScan().then(() => toast("Rescan started"))}>
          <RefreshCw size={13} /> Rescan now
        </Button>
        <Button onClick={() => setS(saved)} disabled={!dirty}>
          Discard
        </Button>
        <Button variant="primary" onClick={save} disabled={!dirty || busy}>
          {busy && <Spinner />} Save changes
        </Button>
      </header>
      <div className="min-h-0 flex-1 overflow-auto">
        <div className="mx-auto flex max-w-[860px] flex-col gap-4 px-6 py-5">
          <Card title="Indexed locations" subtitle="Folders or drives Atlas keeps an index of. Changes trigger a re-index.">
            <PathList
              paths={s.roots}
              onRemove={(p) => set("roots", s.roots.filter((x) => x !== p))}
              empty="Nothing is indexed. Add a folder to get started."
            />
            <Button
              size="sm"
              variant="ghost"
              className="mt-2"
              onClick={async () => {
                const add = await pickFolders();
                set("roots", [...s.roots, ...add.filter((a) => !s.roots.includes(a))]);
              }}
            >
              <FolderPlus size={13} /> Add location
            </Button>
          </Card>

          <Card title="Excluded folders" subtitle="Full paths, or a bare folder name (like node_modules) to skip it everywhere.">
            <PathList paths={s.excluded} onRemove={(p) => set("excluded", s.excluded.filter((x) => x !== p))} empty="No exclusions." />
            <form
              className="mt-2 flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                const v = newExclusion.trim();
                if (v && !s.excluded.includes(v)) set("excluded", [...s.excluded, v]);
                setNewExclusion("");
              }}
            >
              <TextInput value={newExclusion} onChange={(e) => setNewExclusion(e.target.value)} placeholder="node_modules  or  C:\Users\you\Videos\Raw" className="flex-1" />
              <Button type="submit" size="md" disabled={!newExclusion.trim()}>
                Add
              </Button>
              <IconButton
                title="Browse for a folder"
                onClick={async () => {
                  const add = await pickFolders();
                  set("excluded", [...s.excluded, ...add.filter((a) => !s.excluded.includes(a))]);
                }}
              >
                <FolderOpen size={15} />
              </IconButton>
            </form>
          </Card>

          <Card title="Thresholds">
            <div className="grid grid-cols-1 gap-x-6 gap-y-4 sm:grid-cols-2">
              <NumberField label="Inbox age" suffix="days" hint="Downloads and Desktop items older than this appear in the inbox." value={s.inboxDays} onChange={(v) => set("inboxDays", v)} />
              <NumberField label="Work session gap" suffix="minutes" hint="Files touched within this gap are grouped into one session." value={s.sessionGapMin} onChange={(v) => set("sessionGapMin", v)} />
              <NumberField label="Cold file age" suffix="days" hint="Large files untouched this long count as cold." value={s.coldDays} onChange={(v) => set("coldDays", v)} />
              <NumberField label="Cold file minimum size" suffix="MB" hint="Only files at least this large are flagged as large and cold." value={s.coldMinMb} onChange={(v) => set("coldMinMb", v)} />
              <NumberField label="Holding area retention" suffix="days" hint="Cleaned items stay restorable this long, then go to the Recycle Bin." value={s.retentionDays} onChange={(v) => set("retentionDays", Math.max(1, v))} />
              <NumberField label="Duplicate minimum size" suffix="KB" hint="Smaller files are ignored when looking for duplicates." value={s.dupMinKb} onChange={(v) => set("dupMinKb", v)} />
            </div>
          </Card>

          <Card title="Archive folder" subtitle="Inbox “Archive” moves files here, into a folder per month.">
            <div className="flex gap-2">
              <TextInput value={s.archiveDir} onChange={(e) => set("archiveDir", e.target.value)} className="flex-1" />
              <IconButton
                title="Browse"
                onClick={async () => {
                  const p = await open({ directory: true });
                  if (typeof p === "string") set("archiveDir", p);
                }}
              >
                <FolderOpen size={15} />
              </IconButton>
            </div>
          </Card>

          <Card
            title="Cleanup rules"
            subtitle="Patterns for app-generated junk. * matches anything, ? one character. A pattern with \ matches the full path."
            action={
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  api.defaultSettings().then((d) => {
                    set("junkRules", d.junkRules);
                    toast("Default rules restored. Save to apply.");
                  })
                }
              >
                <RotateCcw size={13} /> Reset to defaults
              </Button>
            }
          >
            <div className="grid grid-cols-[24px_minmax(0,1.3fr)_minmax(0,1fr)_96px_28px] items-center gap-2 pb-1 text-[11px] text-ink-3">
              <span />
              <span>Name</span>
              <span>Pattern</span>
              <span>Min. age (days)</span>
              <span />
            </div>
            <div className="space-y-1.5">
              {s.junkRules.map((r, i) => (
                <div key={i} className="grid grid-cols-[24px_minmax(0,1.3fr)_minmax(0,1fr)_96px_28px] items-center gap-2">
                  <input
                    type="checkbox"
                    aria-label={`Enable ${r.name}`}
                    checked={r.enabled}
                    onChange={(e) => setRule(i, { enabled: e.target.checked })}
                    className="accent-[var(--accent)]"
                  />
                  <TextInput value={r.name} onChange={(e) => setRule(i, { name: e.target.value })} className="h-7 text-xs" />
                  <TextInput value={r.pattern} onChange={(e) => setRule(i, { pattern: e.target.value })} className="h-7 font-mono text-xs" spellCheck={false} />
                  <TextInput
                    type="number"
                    min={0}
                    value={r.minAgeDays}
                    onChange={(e) => setRule(i, { minAgeDays: Math.max(0, Number(e.target.value) || 0) })}
                    className="h-7 text-xs"
                  />
                  <IconButton title="Remove rule" onClick={() => set("junkRules", s.junkRules.filter((_, j) => j !== i))}>
                    <X size={13} />
                  </IconButton>
                </div>
              ))}
            </div>
            <Button size="sm" variant="ghost" className="mt-2" onClick={() => set("junkRules", [...s.junkRules, { name: "New rule", pattern: "", enabled: true, minAgeDays: 7 }])}>
              <Plus size={13} /> Add rule
            </Button>
          </Card>

          <Card title="Storage locations">
            <dl className="space-y-2 text-xs">
              <div>
                <dt className="text-ink-3">App data and index</dt>
                <dd className="selectable break-all">{status?.dataDir}</dd>
              </div>
              <div>
                <dt className="text-ink-3">Holding area (system drive)</dt>
                <dd className="selectable break-all">{status?.holdingDir}</dd>
              </div>
              <div className="text-ink-3">On other drives, held items go to a hidden “.atlas-holding” folder at the drive root, so cleaning is always a move on the same drive.</div>
            </dl>
          </Card>
        </div>
      </div>
    </div>
  );
}

function PathList({ paths, onRemove, empty }: { paths: string[]; onRemove: (p: string) => void; empty: string }) {
  if (!paths.length) return <p className="py-1 text-xs text-ink-3">{empty}</p>;
  return (
    <div className="divide-y divide-[var(--border)] rounded-lg border border-line">
      {paths.map((p) => (
        <div key={p} className="flex items-center gap-2 px-3 py-1.5">
          <span className="selectable min-w-0 flex-1 truncate text-[13px]" title={p}>
            {p}
          </span>
          <IconButton title="Remove" onClick={() => onRemove(p)}>
            <X size={13} />
          </IconButton>
        </div>
      ))}
    </div>
  );
}

function NumberField({ label, hint, suffix, value, onChange }: { label: string; hint: string; suffix: string; value: number; onChange: (v: number) => void }) {
  return (
    <label className="block">
      <span className="mb-1 block text-[13px] font-medium">{label}</span>
      <span className="flex items-center gap-2">
        <TextInput type="number" min={0} value={value} onChange={(e) => onChange(Math.max(0, Math.round(Number(e.target.value) || 0)))} className="w-28" />
        <span className="text-xs text-ink-3">{suffix}</span>
      </span>
      <span className="mt-1 block text-xs text-ink-3">{hint}</span>
    </label>
  );
}
