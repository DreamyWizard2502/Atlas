import { open } from "@tauri-apps/plugin-dialog";
import { Brush, Clock, FolderPlus, FolderTree, HardDrive, Layers, LayoutDashboard, Search, Settings, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type Place } from "../lib/api";
import { errorMessage } from "../lib/actions";
import { formatBytes, formatCount } from "../lib/format";
import { navigate, setState, toast, useStore, type Screen } from "../lib/store";
import { Button, Checkbox, IconButton, Spinner } from "./ui";

const NAV: { id: Screen; label: string; icon: typeof Layers }[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "spaces", label: "Spaces", icon: Layers },
  { id: "timeline", label: "Timeline", icon: Clock },
  { id: "cleanup", label: "Cleanup", icon: Brush },
  { id: "folders", label: "Folders", icon: FolderTree },
];

export function Rail() {
  const screen = useStore((s) => s.screen);
  const scanning = useStore((s) => s.scan != null && !s.scan.done);
  const item = (n: { id: Screen; label: string; icon: typeof Layers }) => {
    const active = screen === n.id;
    return (
      <button
        key={n.id}
        type="button"
        title={n.label}
        aria-label={n.label}
        onClick={() => navigate(n.id)}
        className={`relative flex h-10 w-10 items-center justify-center rounded-xl transition-colors duration-100 ${
          active ? "bg-accent-soft text-accent" : "text-ink-3 hover:bg-surface-2 hover:text-ink"
        }`}
      >
        <n.icon size={18} strokeWidth={1.7} />
      </button>
    );
  };
  return (
    <nav className="flex w-[60px] shrink-0 flex-col items-center gap-1.5 border-r border-line bg-surface py-3">
      <div className="mb-3 flex h-9 w-9 items-center justify-center rounded-xl bg-accent text-sm font-semibold text-on-accent" title="Atlas">
        A
      </div>
      <button
        type="button"
        title="Search (Ctrl+K)"
        aria-label="Search"
        onClick={() => setState({ palette: true })}
        className="mb-1 flex h-10 w-10 items-center justify-center rounded-xl text-ink-3 hover:bg-surface-2 hover:text-ink"
      >
        <Search size={18} strokeWidth={1.7} />
      </button>
      {NAV.map(item)}
      <div className="flex-1" />
      {scanning && (
        <div title="Indexing…" className="mb-1 flex h-10 w-10 items-center justify-center">
          <Spinner size={16} />
        </div>
      )}
      {item({ id: "settings", label: "Settings", icon: Settings })}
    </nav>
  );
}

export function ScanBanner() {
  const scan = useStore((s) => s.scan);
  if (!scan || scan.done) return null;
  return (
    <div className="anim-fade flex items-center gap-3 border-b border-line bg-surface px-6 py-2 text-xs">
      <Spinner />
      <span className="font-medium text-ink">{scan.phase === "finalizing" ? "Finalizing index" : "Indexing"}</span>
      <span className="tabular text-ink-2">
        {formatCount(scan.files)} files · {formatBytes(scan.bytes)}
      </span>
      <span className="min-w-0 flex-1 truncate text-ink-3" title={scan.current}>
        {scan.current}
      </span>
      <IconButton title="Cancel indexing" onClick={() => api.cancelScan()}>
        <X size={14} />
      </IconButton>
    </div>
  );
}

export function Onboarding({ onDone }: { onDone: () => void }) {
  const [places, setPlaces] = useState<Place[]>([]);
  const [home, setHome] = useState<string | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    Promise.all([api.places(), api.appStatus()]).then(([p, st]) => {
      setPlaces(p.filter((x) => x.kind === "drive"));
      setHome(st.home);
      if (st.home) setChosen([st.home]);
    });
  }, []);

  const toggle = (p: string, on: boolean) => setChosen((c) => (on ? [...c.filter((x) => x !== p), p] : c.filter((x) => x !== p)));
  const options = [...(home ? [{ path: home, label: "Your user folder", note: "Recommended. Documents, Desktop, Downloads and more." }] : [])];
  for (const d of places) options.push({ path: d.path, label: `Drive ${d.name}`, note: "Whole drive. Takes longer and indexes more noise." });
  for (const c of chosen) if (!options.some((o) => o.path === c)) options.push({ path: c, label: c.split(/[\\/]/).filter(Boolean).pop() ?? c, note: "Custom folder" });

  const addFolder = async () => {
    const picked = await open({ directory: true, multiple: true });
    const list = Array.isArray(picked) ? picked : picked ? [picked] : [];
    setChosen((c) => [...c, ...list.filter((p) => !c.includes(p))]);
  };

  const start = async () => {
    setBusy(true);
    try {
      const s = await api.getSettings();
      await api.saveSettings({ ...s, roots: chosen, onboarded: true });
      onDone();
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center overflow-auto p-8">
      <div className="anim-pop card w-full max-w-[560px] p-7">
        <div className="mb-5 flex h-11 w-11 items-center justify-center rounded-2xl bg-accent text-lg font-semibold text-on-accent">A</div>
        <h1 className="text-xl font-semibold tracking-tight">Welcome to Atlas</h1>
        <p className="mt-1.5 text-[13.5px] text-ink-2">
          Atlas builds a private index of your files so search is instant and the dashboard can show what you're working on, what
          changed, and what's taking up space. Nothing leaves your computer.
        </p>
        <h2 className="mb-2 mt-6 text-[13px] font-semibold">Choose what to index</h2>
        <div className="space-y-1.5">
          {options.map((o) => (
            <label key={o.path} className="flex cursor-default items-start gap-3 rounded-lg border border-line px-3 py-2.5 hover:bg-surface-2">
              <span className="pt-0.5">
                <Checkbox checked={chosen.includes(o.path)} onChange={(on) => toggle(o.path, on)} label={o.label} />
              </span>
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2 text-[13px] font-medium">
                  {o.path.length <= 3 && <HardDrive size={13} className="text-ink-3" />}
                  {o.label}
                </span>
                <span className="block truncate text-xs text-ink-3">{o.path}</span>
                <span className="block text-xs text-ink-3">{o.note}</span>
              </span>
            </label>
          ))}
        </div>
        <div className="mt-3 flex items-center justify-between">
          <Button variant="ghost" onClick={addFolder}>
            <FolderPlus size={14} /> Add a folder
          </Button>
          <Button variant="primary" onClick={start} disabled={!chosen.length || busy}>
            {busy ? <Spinner /> : null} Start indexing
          </Button>
        </div>
        <p className="mt-5 text-xs text-ink-3">
          You can change indexed locations and exclusions any time in Settings. Indexing runs in the background.
        </p>
      </div>
    </div>
  );
}
