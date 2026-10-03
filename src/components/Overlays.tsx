// Context menu, tag / space pickers, confirm dialog and toasts.
import { Archive, Check, Copy, ExternalLink, Eye, FolderOpen, Layers, Plus, Tag, X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type Space, type TagInfo } from "../lib/api";
import { copyPath, errorMessage, moveToHolding, openFile, revealFile } from "../lib/actions";
import { plural } from "../lib/format";
import { bumpData, bumpSpaces, dismissToast, openQuickLook, setState, toast, useStore, type FileRef } from "../lib/store";
import { activate } from "./FileViews";
import { Button, Modal, TextInput } from "./ui";

export function ContextMenu() {
  const menu = useStore((s) => s.menu);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: 0, y: 0 });

  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    setPos({
      x: Math.min(menu.x, window.innerWidth - r.width - 8),
      y: Math.min(menu.y, window.innerHeight - r.height - 8),
    });
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setState({ menu: null });
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", close);
    };
  }, [menu]);

  if (!menu) return null;
  const files = menu.files;
  const one = files[0];
  const isDir = files.some((f) => f.isDir || f.kind === "folder");
  const run = (fn: () => void) => () => {
    setState({ menu: null });
    fn();
  };
  return (
    <div
      ref={ref}
      className="anim-fade fixed z-[60] min-w-[210px] rounded-xl border border-line bg-surface p-1 shadow-[var(--shadow)]"
      style={{ left: pos.x || menu.x, top: pos.y || menu.y }}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      <Item icon={ExternalLink} label={isDir ? "Open folder" : "Open"} onClick={run(() => activate(one))} />
      {!isDir && <Item icon={Eye} label="Quick look" hint="Space" onClick={run(() => openQuickLook(one))} />}
      <Item icon={FolderOpen} label="Reveal in Explorer" onClick={run(() => revealFile(one))} />
      {!isDir && <Item icon={ExternalLink} label="Open with default app" onClick={run(() => openFile(one))} />}
      <Item icon={Copy} label={files.length > 1 ? "Copy paths" : "Copy path"} onClick={run(() => copyPath(files))} />
      <Sep />
      <Item icon={Tag} label="Add tag…" onClick={run(() => setState({ dialog: { kind: "tag", files } }))} />
      <Item icon={Layers} label="Add to space…" onClick={run(() => setState({ dialog: { kind: "space", files } }))} />
      {!isDir && (
        <>
          <Sep />
          <Item icon={Archive} label="Move to holding area" onClick={run(() => moveToHolding(files))} />
        </>
      )}
    </div>
  );
}

function Item({ icon: Icon, label, hint, onClick }: { icon: typeof Tag; label: string; hint?: string; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] hover:bg-surface-2">
      <Icon size={14} strokeWidth={1.7} className="text-ink-3" />
      <span className="flex-1">{label}</span>
      {hint && <span className="text-[11px] text-ink-3">{hint}</span>}
    </button>
  );
}

function Sep() {
  return <div className="my-1 h-px bg-line" />;
}

export function Dialogs() {
  const dialog = useStore((s) => s.dialog);
  const close = () => setState({ dialog: null });
  if (!dialog) return null;
  if (dialog.kind === "tag") return <TagDialog files={dialog.files} onClose={close} />;
  if (dialog.kind === "space") return <SpaceDialog files={dialog.files} fromInbox={dialog.fromInbox} onClose={close} />;
  return (
    <Modal
      title={dialog.title}
      onClose={close}
      footer={
        <>
          <Button onClick={close}>Cancel</Button>
          <Button
            variant="primary"
            onClick={() => {
              close();
              dialog.run();
            }}
          >
            {dialog.confirm}
          </Button>
        </>
      }
    >
      <p className="text-[13px] text-ink-2">{dialog.body}</p>
    </Modal>
  );
}

function TagDialog({ files, onClose }: { files: FileRef[]; onClose: () => void }) {
  const [tags, setTags] = useState<TagInfo[]>([]);
  const [value, setValue] = useState("");
  useEffect(() => {
    api.listTags().then(setTags).catch(() => {});
  }, []);
  const apply = async (tag: string) => {
    if (!tag.trim()) return;
    try {
      await api.addTag(files.map((f) => f.path), tag.trim());
      toast(`Tagged ${plural(files.length, "file")} “${tag.trim()}”`);
      bumpData();
      onClose();
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
    }
  };
  const matches = tags.filter((t) => t.name.toLowerCase().includes(value.toLowerCase()));
  return (
    <Modal title={`Add tag to ${files.length === 1 ? files[0].name : plural(files.length, "file")}`} onClose={onClose} width={400}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          apply(value);
        }}
        className="flex gap-2"
      >
        <TextInput autoFocus value={value} onChange={(e) => setValue(e.target.value)} placeholder="Tag name" className="flex-1" />
        <Button type="submit" variant="primary" disabled={!value.trim()}>
          Add
        </Button>
      </form>
      {matches.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-1.5">
          {matches.slice(0, 30).map((t) => (
            <button
              key={t.id}
              type="button"
              onClick={() => apply(t.name)}
              className="rounded-full border border-line px-2.5 py-0.5 text-xs text-ink-2 hover:border-accent hover:text-accent"
            >
              {t.name} <span className="text-ink-3">{t.count}</span>
            </button>
          ))}
        </div>
      )}
    </Modal>
  );
}

function SpaceDialog({ files, fromInbox, onClose }: { files: FileRef[]; fromInbox?: boolean; onClose: () => void }) {
  const [spaces, setSpaces] = useState<Space[] | null>(null);
  const [name, setName] = useState("");
  useEffect(() => {
    api.listSpaces().then(setSpaces).catch(() => setSpaces([]));
  }, []);
  const add = async (id: number, label: string) => {
    try {
      await api.addToSpace(id, files.map((f) => f.path));
      // Filing an inbox item into a space counts as triaging it.
      if (fromInbox) await api.inboxDismiss(files.map((f) => f.path));
      toast(`Added ${plural(files.length, "item")} to ${label}`);
      bumpSpaces();
      bumpData();
      onClose();
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
    }
  };
  const create = async () => {
    if (!name.trim()) return;
    try {
      const id = await api.saveSpace({ name: name.trim(), rules: { mode: "all", rules: [] }, pinned: false });
      await add(id, name.trim());
    } catch (e) {
      toast(errorMessage(e), { tone: "attention" });
    }
  };
  return (
    <Modal title={`Add ${files.length === 1 ? files[0].name : plural(files.length, "item")} to a space`} onClose={onClose} width={420}>
      <p className="mb-3 text-xs text-ink-3">Files stay where they are; a space just collects them.</p>
      <div className="mb-3 max-h-64 space-y-0.5 overflow-auto">
        {spaces?.map((s) => (
          <button
            key={s.id}
            type="button"
            onClick={() => add(s.id, s.name)}
            className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] hover:bg-surface-2"
          >
            <Layers size={14} className="text-accent" />
            <span className="flex-1 truncate">{s.name}</span>
            <span className="text-xs text-ink-3">{plural(s.count, "file")}</span>
          </button>
        ))}
        {spaces?.length === 0 && <p className="py-2 text-xs text-ink-3">No spaces yet.</p>}
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          create();
        }}
        className="flex gap-2 border-t border-line pt-3"
      >
        <TextInput value={name} onChange={(e) => setName(e.target.value)} placeholder="New space name" className="flex-1" />
        <Button type="submit" disabled={!name.trim()}>
          <Plus size={14} /> Create and add
        </Button>
      </form>
    </Modal>
  );
}

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[70] flex w-[360px] flex-col gap-2">
      {toasts.map((t) => (
        <div key={t.id} className="anim-pop card pointer-events-auto flex items-start gap-2.5 px-3.5 py-2.5 text-[13px] shadow-[var(--shadow)]">
          {t.tone === "success" && <Check size={15} className="mt-0.5 shrink-0 text-green" />}
          {t.tone === "attention" && <span className="mt-1.5 h-2 w-2 shrink-0 rounded-full bg-amber-mark" />}
          <span className="flex-1 text-ink">{t.message}</span>
          {t.action && (
            <button
              type="button"
              className="shrink-0 font-medium text-accent hover:underline"
              onClick={() => {
                t.action!.run();
                dismissToast(t.id);
              }}
            >
              {t.action.label}
            </button>
          )}
          <button type="button" aria-label="Dismiss" className="shrink-0 text-ink-3 hover:text-ink" onClick={() => dismissToast(t.id)}>
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
