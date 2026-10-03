// File actions shared by every view (context menu, Quick Look, palette).
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api } from "./api";
import { formatBytes, plural } from "./format";
import { bumpData, setState, toast, type FileRef } from "./store";

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export async function openFile(f: FileRef) {
  try {
    await api.openFile(f.path);
  } catch (e) {
    toast(`Couldn't open ${f.name}: ${errorMessage(e)}`, { tone: "attention" });
  }
}

export async function revealFile(f: FileRef) {
  try {
    await api.revealFile(f.path);
  } catch (e) {
    toast(`Couldn't reveal ${f.name}: ${errorMessage(e)}`, { tone: "attention" });
  }
}

export async function copyPath(files: FileRef[]) {
  try {
    await writeText(files.map((f) => f.path).join("\r\n"));
    toast(files.length === 1 ? "Path copied" : `${files.length} paths copied`);
  } catch (e) {
    toast(`Couldn't copy: ${errorMessage(e)}`, { tone: "attention" });
  }
}

/** Restore everything moved in one batch (used by toast "Restore" actions). */
export async function restoreBatch(batchId: string) {
  const held = await api.holdingList(false);
  const ids = held.filter((h) => h.batchId === batchId).map((h) => h.id);
  if (!ids.length) return;
  const out = await api.holdingRestore(ids);
  const ok = out.filter((o) => o.ok).length;
  toast(`Restored ${plural(ok, "item")}`, { tone: "success" });
  bumpData();
}

export async function moveToHolding(files: FileRef[]) {
  try {
    const report = await api.moveToHolding(files.map((f) => f.path));
    if (report.moved > 0) {
      toast(`Moved ${plural(report.moved, "item")} (${formatBytes(report.bytes)}) to the holding area`, {
        tone: "success",
        action: { label: "Restore", run: () => restoreBatch(report.batchId) },
      });
    }
    const skipped = report.outcomes.filter((o) => !o.ok);
    if (skipped.length) {
      toast(`Skipped ${plural(skipped.length, "item")}: ${skipped[0].detail}`, { tone: "attention" });
    }
    setState({ quickLook: false });
    bumpData();
  } catch (e) {
    toast(errorMessage(e), { tone: "attention" });
  }
}

export function toRef(f: {
  path: string;
  name: string;
  kind: FileRef["kind"];
  ext?: string;
  isDir?: boolean;
  size?: number | null;
  modified?: number | null;
}): FileRef {
  return { path: f.path, name: f.name, kind: f.kind, ext: f.ext, isDir: f.isDir, size: f.size, modified: f.modified };
}
