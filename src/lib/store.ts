// Minimal global store (useSyncExternalStore). Holds navigation, the current
// selection (for Quick Look / context menu), overlays, toasts and a data
// version counter that widgets watch to refetch after index changes.
import { useSyncExternalStore } from "react";
import type { AppStatus, Kind, ScanProgress } from "./api";

export type Screen = "dashboard" | "spaces" | "timeline" | "cleanup" | "folders" | "settings";

export interface FileRef {
  path: string;
  name: string;
  kind: Kind;
  ext?: string;
  isDir?: boolean;
  size?: number | null;
  modified?: number | null;
}

export interface Toast {
  id: number;
  message: string;
  tone?: "default" | "success" | "attention";
  action?: { label: string; run: () => void };
}

export type Dialog =
  | { kind: "tag"; files: FileRef[] }
  | { kind: "space"; files: FileRef[]; fromInbox?: boolean }
  | { kind: "confirm"; title: string; body: string; confirm: string; run: () => void };

interface State {
  screen: Screen;
  spaceId: number | null;
  folderPath: string | null;
  selection: FileRef | null;
  selectionList: FileRef[];
  quickLook: boolean;
  palette: boolean;
  menu: { x: number; y: number; files: FileRef[] } | null;
  dialog: Dialog | null;
  toasts: Toast[];
  dataVersion: number;
  spacesVersion: number;
  scan: ScanProgress | null;
  status: AppStatus | null;
}

let state: State = {
  screen: "dashboard",
  spaceId: null,
  folderPath: null,
  selection: null,
  selectionList: [],
  quickLook: false,
  palette: false,
  menu: null,
  dialog: null,
  toasts: [],
  dataVersion: 0,
  spacesVersion: 0,
  scan: null,
  status: null,
};

const listeners = new Set<() => void>();

export function getState() {
  return state;
}

export function setState(patch: Partial<State> | ((s: State) => Partial<State>)) {
  const p = typeof patch === "function" ? patch(state) : patch;
  state = { ...state, ...p };
  listeners.forEach((l) => l());
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useStore<T>(selector: (s: State) => T): T {
  return useSyncExternalStore(subscribe, () => selector(state));
}

// ---- actions ----

export function navigate(screen: Screen, params: { spaceId?: number | null; folderPath?: string | null } = {}) {
  setState({ screen, palette: false, menu: null, ...params });
}

export function select(file: FileRef | null, list?: FileRef[]) {
  setState((s) => ({ selection: file, selectionList: list ?? s.selectionList }));
}

export function openQuickLook(file?: FileRef, list?: FileRef[]) {
  if (file) select(file, list);
  if (getState().selection) setState({ quickLook: true, menu: null });
}

export function bumpData() {
  setState((s) => ({ dataVersion: s.dataVersion + 1 }));
}

export function bumpSpaces() {
  setState((s) => ({ spacesVersion: s.spacesVersion + 1 }));
}

let toastId = 0;
export function toast(message: string, opts: Omit<Toast, "id" | "message"> = {}) {
  const id = ++toastId;
  setState((s) => ({ toasts: [...s.toasts.slice(-3), { id, message, ...opts }] }));
  setTimeout(() => dismissToast(id), opts.action ? 8000 : 4000);
}

export function dismissToast(id: number) {
  setState((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
}

export function openMenu(e: { clientX: number; clientY: number; preventDefault: () => void }, files: FileRef[]) {
  e.preventDefault();
  if (files[0]) select(files[0]);
  setState({ menu: { x: e.clientX, y: e.clientY, files } });
}
