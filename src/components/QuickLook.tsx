// Spacebar Quick Look: images, PDF, text/code, video, audio; metadata card for
// everything else. The preview mode comes from the file's *contents* (backend
// probe), never its extension alone, and every failure (unexpected bytes, a
// file that vanishes or is locked mid-read, a render error) degrades to the
// metadata card. Esc closes, ←/→ step through the list it was opened from.
import { ChevronLeft, ChevronRight, ExternalLink, FolderOpen, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, assetUrl, type FileDetails, type Probe } from "../lib/api";
import { errorMessage, openFile, revealFile } from "../lib/actions";
import { formatBytes, formatDate, formatCount } from "../lib/format";
import { select, setState, useStore, type FileRef } from "../lib/store";
import { ErrorBoundary } from "./ErrorBoundary";
import { FileIcon, kindLabel } from "./FileIcon";
import { IconButton, Pill, Spinner } from "./ui";

/** Lines rendered in a text preview (the backend already caps bytes). */
const MAX_LINES = 4000;

export function QuickLook() {
  const open = useStore((s) => s.quickLook);
  const file = useStore((s) => s.selection);
  if (!open || !file) return null;
  return <QuickLookInner file={file} />;
}

function QuickLookInner({ file }: { file: FileRef }) {
  const list = useStore((s) => s.selectionList);
  const idx = list.findIndex((f) => f.path === file.path);
  const close = () => setState({ quickLook: false });
  const step = (d: number) => {
    if (idx < 0) return;
    const next = list[idx + d];
    if (next) select(next);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === " ") {
        e.preventDefault();
        e.stopPropagation();
        close();
      } else if (e.key === "ArrowRight" || e.key === "ArrowDown") {
        e.preventDefault();
        step(1);
      } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
        e.preventDefault();
        step(-1);
      } else if (e.key === "Enter") {
        e.preventDefault();
        openFile(file);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  return (
    <div className="anim-fade fixed inset-0 z-50 flex items-center justify-center bg-[var(--overlay)] p-8" onMouseDown={close}>
      <div
        className="anim-pop card flex h-full max-h-[86vh] w-full max-w-[1100px] flex-col overflow-hidden shadow-[var(--shadow)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="flex items-center gap-3 border-b border-line px-4 py-2.5">
          <FileIcon kind={file.kind} />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[13.5px] font-semibold">{file.name}</div>
          </div>
          {list.length > 1 && idx >= 0 && (
            <div className="flex items-center gap-1 text-xs text-ink-3">
              <IconButton title="Previous (←)" onClick={() => step(-1)} disabled={idx <= 0}>
                <ChevronLeft size={15} />
              </IconButton>
              <span className="tabular">
                {idx + 1} / {list.length}
              </span>
              <IconButton title="Next (→)" onClick={() => step(1)} disabled={idx >= list.length - 1}>
                <ChevronRight size={15} />
              </IconButton>
            </div>
          )}
          <IconButton title="Open with default app (Enter)" onClick={() => openFile(file)}>
            <ExternalLink size={15} />
          </IconButton>
          <IconButton title="Reveal in Explorer" onClick={() => revealFile(file)}>
            <FolderOpen size={15} />
          </IconButton>
          <IconButton title="Close (Esc)" onClick={close}>
            <X size={15} />
          </IconButton>
        </header>
        <div className="flex min-h-0 flex-1">
          <div className="flex min-w-0 flex-1 items-center justify-center bg-surface-2">
            <ErrorBoundary resetKey={file.path} fallback={() => <MetaFallback file={file} note="Atlas couldn't show a preview of this file." />}>
              <Preview key={file.path} file={file} />
            </ErrorBoundary>
          </div>
          <ErrorBoundary resetKey={file.path} fallback={() => <aside className="w-[260px] shrink-0 border-l border-line p-4 text-xs text-ink-3">Details unavailable.</aside>}>
            <Details key={`d-${file.path}`} file={file} />
          </ErrorBoundary>
        </div>
      </div>
    </div>
  );
}

function Preview({ file }: { file: FileRef }) {
  const [probe, setProbe] = useState<Probe | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    api
      .probe(file.path)
      .then((p) => alive && setProbe(p))
      .catch((e) => alive && setProbe({ exists: false, isDir: false, size: 0, mode: "none", kind: file.kind, note: errorMessage(e) }));
    return () => {
      alive = false;
    };
  }, [file.path, file.kind]);
  if (!probe) return <Spinner size={18} />;
  const shown = { ...file, kind: probe.kind };
  if (failed) return <MetaFallback file={shown} note="This file couldn't be previewed. It may have changed or been removed." />;
  const src = assetUrl(file.path);
  switch (probe.mode) {
    case "image":
      return <img src={src} alt="" draggable={false} onError={() => setFailed(true)} className="max-h-full max-w-full object-contain p-4" />;
    case "video":
      return <video src={src} controls autoPlay onError={() => setFailed(true)} className="max-h-full max-w-full" />;
    case "audio":
      return (
        <div className="flex flex-col items-center gap-6">
          <FileIcon kind="audio" size={64} />
          <audio src={src} controls autoPlay onError={() => setFailed(true)} />
        </div>
      );
    case "pdf":
      return <iframe src={src} title={file.name} onError={() => setFailed(true)} className="h-full w-full border-0 bg-surface" />;
    case "text":
      return <TextPreview file={shown} />;
    default:
      return <MetaFallback file={shown} note={probe.note ?? undefined} />;
  }
}

function TextPreview({ file }: { file: FileRef }) {
  const [state, setStateLocal] = useState<{ text: string; truncated: boolean } | { error: string } | null>(null);
  useEffect(() => {
    api.readText(file.path).then(setStateLocal).catch((e) => setStateLocal({ error: errorMessage(e) }));
  }, [file.path]);
  if (!state) return <Spinner size={18} />;
  if ("error" in state) return <MetaFallback file={file} note={state.error} />;
  const all = state.text.split(/\r?\n/);
  const lines = all.slice(0, MAX_LINES);
  return (
    <div className="selectable h-full w-full overflow-auto bg-surface">
      <pre className="min-w-fit p-4 font-mono text-[12px] leading-[1.6] text-ink">
        {lines.map((l, i) => (
          <div key={i} className="flex">
            <span className="tabular mr-4 inline-block w-10 shrink-0 select-none text-right text-ink-3">{i + 1}</span>
            <span className="whitespace-pre">{l || " "}</span>
          </div>
        ))}
        {(state.truncated || all.length > MAX_LINES) && <div className="mt-3 text-ink-3">… preview truncated</div>}
      </pre>
    </div>
  );
}

function MetaFallback({ file, note }: { file: FileRef; note?: string }) {
  return (
    <div className="flex flex-col items-center gap-3 p-8 text-center">
      <FileIcon kind={file.kind} size={72} />
      <div className="text-sm font-medium">{file.name}</div>
      <div className="text-xs text-ink-3">{note ?? `${kindLabel(file.kind, file.ext)} · no preview available`}</div>
    </div>
  );
}

function Details({ file }: { file: FileRef }) {
  const [d, setD] = useState<FileDetails | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    api.fileDetails(file.path).then(setD).catch((e) => setErr(errorMessage(e)));
  }, [file.path]);
  return (
    <aside className="w-[260px] shrink-0 overflow-auto border-l border-line p-4 text-xs">
      {err && <p className="text-ink-2">{err}</p>}
      {!d && !err && <Spinner />}
      {d && (
        <dl className="space-y-3">
          <Field label="Kind">{kindLabel(d.kind, d.ext)}</Field>
          <Field label="Size">
            {formatBytes(d.size)}
            {d.fileCount != null && <span className="text-ink-3"> · {formatCount(d.fileCount)} files</span>}
          </Field>
          <Field label="Modified">{formatDate(d.modified)}</Field>
          <Field label="Created">{formatDate(d.created)}</Field>
          <Field label="Location">
            <span className="selectable break-all">{d.parent}</span>
          </Field>
          {d.tags.length > 0 && (
            <Field label="Tags">
              <div className="flex flex-wrap gap-1">
                {d.tags.map((t) => (
                  <Pill key={t} tone="accent">
                    {t}
                  </Pill>
                ))}
              </div>
            </Field>
          )}
          {d.spaces.length > 0 && (
            <Field label="In spaces">
              <div className="flex flex-wrap gap-1">
                {d.spaces.map((t) => (
                  <Pill key={t}>{t}</Pill>
                ))}
              </div>
            </Field>
          )}
        </dl>
      )}
    </aside>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="mb-0.5 text-[11px] text-ink-3">{label}</dt>
      <dd className="text-ink">{children}</dd>
    </div>
  );
}
