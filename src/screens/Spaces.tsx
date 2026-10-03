import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, LayoutGrid, Layers, List, Pencil, Pin, PinOff, Plus, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { FileGrid, FileTable, type FileLike } from "../components/FileViews";
import { Button, Empty, IconButton, Modal, Segmented, Select, Spinner, TextInput } from "../components/ui";
import { api, type Space, type SpaceInput, type SpaceRule } from "../lib/api";
import { errorMessage, toRef } from "../lib/actions";
import { formatBytes, formatCount, plural, relativeTime } from "../lib/format";
import { useLoad } from "../lib/hooks";
import { bumpSpaces, navigate, setState, toast, useStore } from "../lib/store";

const FIELDS: { id: string; label: string; placeholder: string; input?: "number" | "date" | "folder" | "kind" }[] = [
  { id: "nameContains", label: "Name contains", placeholder: "e.g. invoice" },
  { id: "kind", label: "File type", placeholder: "", input: "kind" },
  { id: "ext", label: "Extension", placeholder: "e.g. sldprt, step" },
  { id: "folder", label: "Inside folder", placeholder: "C:\\Users\\you\\Projects", input: "folder" },
  { id: "tag", label: "Has tag", placeholder: "e.g. client" },
  { id: "modifiedWithinDays", label: "Modified in the last (days)", placeholder: "30", input: "number" },
  { id: "modifiedAfter", label: "Modified after", placeholder: "", input: "date" },
  { id: "modifiedBefore", label: "Modified before", placeholder: "", input: "date" },
  { id: "sizeAboveMb", label: "Larger than (MB)", placeholder: "100", input: "number" },
];

const KINDS = ["image", "video", "audio", "pdf", "doc", "sheet", "slides", "text", "code", "archive", "cad", "installer", "font"];

const TEMPLATES: SpaceInput[] = [
  { name: "CAD work", rules: { mode: "all", rules: [{ field: "kind", value: "cad" }] } },
  { name: "This week", rules: { mode: "all", rules: [{ field: "modifiedWithinDays", value: "7" }] } },
  { name: "Invoices", rules: { mode: "all", rules: [{ field: "nameContains", value: "invoice" }, { field: "kind", value: "pdf" }] } },
  { name: "Photos", rules: { mode: "all", rules: [{ field: "kind", value: "image" }, { field: "sizeAboveMb", value: "0.5" }] } },
];

export function describeRules(s: { rules: { mode: string; rules: SpaceRule[] } }): string {
  if (!s.rules.rules.length) return "Files you add manually";
  const parts = s.rules.rules.map((r) => {
    const f = FIELDS.find((x) => x.id === r.field);
    return `${(f?.label ?? r.field).toLowerCase()} ${r.value}`;
  });
  return parts.join(s.rules.mode === "any" ? " or " : " and ");
}

export function Spaces() {
  const spaceId = useStore((s) => s.spaceId);
  const spacesVersion = useStore((s) => s.spacesVersion);
  const { data: spaces, loading } = useLoad(api.listSpaces, [spacesVersion]);
  const [editing, setEditing] = useState<SpaceInput | null>(null);
  const current = spaces?.find((s) => s.id === spaceId) ?? spaces?.[0] ?? null;

  const save = async (input: SpaceInput) => {
    try {
      const id = await api.saveSpace(input);
      setEditing(null);
      bumpSpaces();
      navigate("spaces", { spaceId: id });
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
    }
  };

  return (
    <div className="flex h-full">
      <aside className="flex w-[260px] shrink-0 flex-col border-r border-line bg-surface">
        <div className="flex items-center justify-between px-4 pb-2 pt-4">
          <h1 className="text-[15px] font-semibold">Spaces</h1>
          <IconButton title="New space" onClick={() => setEditing({ name: "", rules: { mode: "all", rules: [{ field: "nameContains", value: "" }] }, pinned: true })}>
            <Plus size={16} />
          </IconButton>
        </div>
        <p className="px-4 pb-3 text-xs text-ink-3">Smart collections built from rules. A file can live in many spaces without moving.</p>
        <div className="min-h-0 flex-1 overflow-auto px-2 pb-3">
          {loading && !spaces && <Spinner />}
          {spaces?.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => navigate("spaces", { spaceId: s.id })}
              className={`flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left transition-colors duration-100 ${
                current?.id === s.id ? "bg-accent-soft" : "hover:bg-surface-2"
              }`}
            >
              <Layers size={15} className={current?.id === s.id ? "text-accent" : "text-ink-3"} />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-medium">{s.name}</div>
                <div className="truncate text-[11.5px] text-ink-3">
                  {plural(s.count, "file")} · {formatBytes(s.size)}
                </div>
              </div>
              {s.pinned && <Pin size={12} className="shrink-0 text-ink-3" />}
            </button>
          ))}
        </div>
      </aside>
      <section className="min-w-0 flex-1">
        {current ? (
          <SpaceDetail key={current.id} space={current} onEdit={() => setEditing({ id: current.id, name: current.name, rules: current.rules, pinned: current.pinned, icon: current.icon })} />
        ) : spaces && !loading ? (
          <div className="flex h-full items-center justify-center">
            <div className="max-w-md text-center">
              <Layers size={28} className="mx-auto mb-3 text-ink-3" />
              <h2 className="text-[15px] font-semibold">Collect files by intent, not by folder</h2>
              <p className="mt-1 text-[13px] text-ink-2">
                A space gathers files that match rules — a project name, a file type, a tag, a date range — wherever they live.
              </p>
              <div className="mt-5 flex flex-wrap justify-center gap-2">
                {TEMPLATES.map((t) => (
                  <Button key={t.name} onClick={() => save({ ...t, pinned: true })}>
                    <Plus size={13} /> {t.name}
                  </Button>
                ))}
              </div>
              <Button className="mt-3" variant="primary" onClick={() => setEditing({ name: "", rules: { mode: "all", rules: [{ field: "nameContains", value: "" }] }, pinned: true })}>
                New space from scratch
              </Button>
            </div>
          </div>
        ) : null}
      </section>
      {editing && <SpaceEditor initial={editing} onClose={() => setEditing(null)} onSave={save} />}
    </div>
  );
}

function SpaceDetail({ space, onEdit }: { space: Space; onEdit: () => void }) {
  const home = useStore((s) => s.status?.home);
  const [view, setView] = useState<"grid" | "list">("grid");
  const [sort, setSort] = useState("recent");
  const { data, loading } = useLoad(() => api.spaceFiles(space.id, sort, 5000), [space.id, sort]);
  const items: FileLike[] = useMemo(() => (data?.items ?? []).map((f) => ({ ...toRef(f), parent: f.parent })), [data]);

  const remove = () =>
    setState({
      dialog: {
        kind: "confirm",
        title: `Delete “${space.name}”?`,
        body: "Only the space is deleted. The files themselves are not touched.",
        confirm: "Delete space",
        run: async () => {
          await api.deleteSpace(space.id);
          bumpSpaces();
          navigate("spaces", { spaceId: null });
        },
      },
    });

  const empty = (
    <Empty
      title={loading ? "Loading…" : "No files match yet"}
      body={space.rules.rules.length ? "Edit the rules, or right-click any file and choose “Add to space”." : "Right-click any file and choose “Add to space”."}
    />
  );

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-start gap-4 border-b border-line px-6 pb-4 pt-5">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-lg font-semibold tracking-tight">{space.name}</h1>
          <p className="mt-0.5 truncate text-xs text-ink-3">
            {describeRules(space)} · {plural(data?.total ?? space.count, "file")} · {formatBytes(space.size)}
            {space.lastActivity ? ` · active ${relativeTime(space.lastActivity)}` : ""}
          </p>
        </div>
        <Select value={sort} onChange={(e) => setSort(e.target.value)} aria-label="Sort">
          <option value="recent">Recent activity</option>
          <option value="name">Name</option>
          <option value="size">Size</option>
        </Select>
        <Segmented
          value={view}
          onChange={setView}
          options={[
            { value: "grid", label: <LayoutGrid size={13} />, title: "Cards" },
            { value: "list", label: <List size={13} />, title: "List" },
          ]}
        />
        <IconButton
          title={space.pinned ? "Unpin from dashboard" : "Pin to dashboard"}
          active={space.pinned}
          onClick={async () => {
            await api.pinSpace(space.id, !space.pinned);
            bumpSpaces();
          }}
        >
          {space.pinned ? <PinOff size={15} /> : <Pin size={15} />}
        </IconButton>
        <IconButton title="Edit rules" onClick={onEdit}>
          <Pencil size={15} />
        </IconButton>
        <IconButton title="Delete space" onClick={remove}>
          <X size={15} />
        </IconButton>
      </header>
      <div className="min-h-0 flex-1">
        {view === "grid" ? (
          <FileGrid items={items} emptyState={empty} />
        ) : (
          <FileTable items={items} showLocation home={home} emptyState={empty} />
        )}
      </div>
      {data && data.total > items.length && (
        <div className="border-t border-line px-6 py-2 text-xs text-ink-3">
          Showing the {formatCount(items.length)} most relevant of {formatCount(data.total)} files. Narrow the rules to see the rest.
        </div>
      )}
    </div>
  );
}

function SpaceEditor({ initial, onClose, onSave }: { initial: SpaceInput; onClose: () => void; onSave: (s: SpaceInput) => void }) {
  const [name, setName] = useState(initial.name);
  const [mode, setMode] = useState<"all" | "any">(initial.rules.mode);
  const [rules, setRules] = useState<SpaceRule[]>(initial.rules.rules);
  const [pinned, setPinned] = useState(initial.pinned ?? false);
  const [tags, setTags] = useState<string[]>([]);
  useEffect(() => {
    api.listTags().then((t) => setTags(t.map((x) => x.name))).catch(() => {});
  }, []);
  const update = (i: number, patch: Partial<SpaceRule>) => setRules((r) => r.map((x, j) => (j === i ? { ...x, ...patch } : x)));
  const submit = () =>
    onSave({ id: initial.id ?? null, name: name.trim(), icon: initial.icon, pinned, rules: { mode, rules: rules.filter((r) => r.value.trim()) } });

  return (
    <Modal
      title={initial.id ? "Edit space" : "New space"}
      onClose={onClose}
      width={620}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={submit} disabled={!name.trim()}>
            Save space
          </Button>
        </>
      }
    >
      <label className="mb-1 block text-xs text-ink-2">Name</label>
      <TextInput autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Bracket redesign" className="w-full" />
      <div className="mb-2 mt-5 flex items-center gap-2 text-xs text-ink-2">
        Include files that match
        <Select value={mode} onChange={(e) => setMode(e.target.value as "all" | "any")} className="h-7 text-xs">
          <option value="all">all of</option>
          <option value="any">any of</option>
        </Select>
        these rules
      </div>
      <div className="space-y-2">
        {rules.map((r, i) => {
          const f = FIELDS.find((x) => x.id === r.field) ?? FIELDS[0];
          return (
            <div key={i} className="flex items-center gap-2">
              <Select value={r.field} onChange={(e) => update(i, { field: e.target.value, value: "" })} className="w-[200px]">
                {FIELDS.map((x) => (
                  <option key={x.id} value={x.id}>
                    {x.label}
                  </option>
                ))}
              </Select>
              {f.input === "kind" ? (
                <Select value={r.value} onChange={(e) => update(i, { value: e.target.value })} className="flex-1">
                  <option value="">Choose a type</option>
                  {KINDS.map((k) => (
                    <option key={k} value={k}>
                      {k}
                    </option>
                  ))}
                </Select>
              ) : (
                <TextInput
                  value={r.value}
                  type={f.input === "number" ? "number" : f.input === "date" ? "date" : "text"}
                  list={r.field === "tag" ? "space-tags" : undefined}
                  onChange={(e) => update(i, { value: e.target.value })}
                  placeholder={f.placeholder}
                  className="min-w-0 flex-1"
                />
              )}
              {f.input === "folder" && (
                <IconButton
                  title="Browse"
                  onClick={async () => {
                    const p = await open({ directory: true });
                    if (typeof p === "string") update(i, { value: p });
                  }}
                >
                  <FolderOpen size={15} />
                </IconButton>
              )}
              <IconButton title="Remove rule" onClick={() => setRules((rs) => rs.filter((_, j) => j !== i))}>
                <X size={14} />
              </IconButton>
            </div>
          );
        })}
        <datalist id="space-tags">
          {tags.map((t) => (
            <option key={t} value={t} />
          ))}
        </datalist>
      </div>
      <Button size="sm" variant="ghost" className="mt-2" onClick={() => setRules((r) => [...r, { field: "nameContains", value: "" }])}>
        <Plus size={13} /> Add rule
      </Button>
      <label className="mt-5 flex items-center gap-2 text-[13px]">
        <input type="checkbox" checked={pinned} onChange={(e) => setPinned(e.target.checked)} className="accent-[var(--accent)]" />
        Pin to the dashboard
      </label>
      <p className="mt-3 text-xs text-ink-3">Files you add with “Add to space” are always included, whatever the rules.</p>
    </Modal>
  );
}
