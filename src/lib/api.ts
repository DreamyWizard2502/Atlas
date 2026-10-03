// Typed wrappers around the Rust commands. The frontend never touches the disk;
// everything goes through here (media previews use the read-only asset protocol).
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Kind =
  | "folder" | "image" | "video" | "audio" | "pdf" | "doc" | "sheet" | "slides"
  | "text" | "code" | "archive" | "cad" | "installer" | "font" | "other";

export interface FileItem {
  id: number;
  path: string;
  parent: string;
  name: string;
  ext: string;
  isDir: boolean;
  size: number;
  created: number | null;
  modified: number | null;
  accessed: number | null;
  kind: Kind;
}

export interface ParsedQuery {
  text: string[];
  exts: string[];
  sizeMin: number | null;
  sizeMax: number | null;
  modifiedFrom: number | null;
  modifiedTo: number | null;
  modifiedLabel: string | null;
  space: string | null;
  foldersOnly: boolean | null;
  chips: string[];
}

export interface SearchResult {
  items: FileItem[];
  parsed: ParsedQuery;
  unknownSpace: boolean;
  tookMs: number;
}

export interface DiskSpace { mount: string; total: number; free: number }

export interface Metrics {
  drive: DiskSpace | null;
  indexedBytes: number;
  indexedFiles: number;
  growthMonth: number | null;
  growthSince: string | null;
  inboxCount: number;
  inboxBytes: number;
  reclaimable: number | null;
  reclaimableAt: number | null;
}

export type RecentFile = FileItem & { lastTouched: number; opened: boolean };
export interface Session { start: number; end: number; files: RecentFile[] }

export interface TreeNode {
  path: string;
  name: string;
  size: number;
  isDir: boolean;
  kind: Kind;
  fileCount: number;
  other: boolean;
}
export interface Treemap {
  path: string | null;
  name: string;
  size: number;
  crumbs: [string, string][];
  children: TreeNode[];
}

export interface Growth {
  points: { day: string; size: number }[];
  fastest: { path: string; name: string; delta: number; size: number; since: string } | null;
  topTypes: [string, number, number][];
}

export type InboxItem = FileItem & {
  source: "Downloads" | "Desktop";
  ageDays: number;
  installedAs: string | null;
  duplicateOf: string | null;
};
export interface Inbox { items: InboxItem[]; total: number; totalSize: number; thresholdDays: number }

export interface FileDetails {
  path: string;
  name: string;
  parent: string;
  ext: string;
  kind: Kind;
  isDir: boolean;
  size: number;
  fileCount: number | null;
  created: number | null;
  modified: number | null;
  accessed: number | null;
  readonly: boolean;
  hidden: boolean;
  tags: string[];
  spaces: string[];
}

export interface TagInfo { id: number; name: string; count: number }

export interface SpaceRule { field: string; value: string }
export interface SpaceRules { mode: "all" | "any"; rules: SpaceRule[] }
export interface Space {
  id: number;
  name: string;
  icon: string;
  rules: SpaceRules;
  pinned: boolean;
  count: number;
  size: number;
  lastActivity: number | null;
}
export interface SpaceInput { id?: number | null; name: string; icon?: string; rules: SpaceRules; pinned?: boolean }

export interface TimelineEntry {
  ts: number;
  change: "created" | "modified" | "deleted";
  path: string;
  name: string;
  ext: string;
  size: number;
  kind: Kind;
  exists: boolean;
}

export interface Place { name: string; path: string; kind: "known" | "root" | "drive" }
export interface DirEntry {
  path: string;
  name: string;
  ext: string;
  isDir: boolean;
  size: number | null;
  modified: number | null;
  created: number | null;
  hidden: boolean;
  kind: Kind;
}

export interface CategorySummary { id: string; label: string; count: number; reclaimable: number; truncated: boolean }
export interface CleanupSummary { analyzedAt: number; totalReclaimable: number; categories: CategorySummary[] }
export interface CleanupState { summary: CleanupSummary | null; hasItems: boolean; analyzing: boolean }
export interface Candidate {
  path: string;
  name: string;
  size: number;
  category: string;
  reason: string;
  modified: number | null;
  isDir: boolean;
  group: number | null;
}
export interface CleanItem { path: string; category: string; size: number }
export interface ItemOutcome { path: string; ok: boolean; detail: string }
export interface RunReport { batchId: string; dryRun: boolean; moved: number; skipped: number; bytes: number; outcomes: ItemOutcome[] }
export interface HeldItem {
  id: number;
  batchId: string;
  originalPath: string;
  heldPath: string;
  size: number;
  category: string;
  isDir: boolean;
  heldAt: number;
  expiresAt: number;
  status: "held" | "restored" | "trashed" | "missing";
}
export interface LogEntry { id: number; ts: number; batchId: string; action: string; path: string; detail: string; dryRun: boolean }

export interface JunkRule { name: string; pattern: string; enabled: boolean; minAgeDays: number }
export interface Settings {
  roots: string[];
  excluded: string[];
  inboxDays: number;
  coldDays: number;
  coldMinMb: number;
  retentionDays: number;
  dupMinKb: number;
  sessionGapMin: number;
  archiveDir: string;
  junkRules: JunkRule[];
  onboarded: boolean;
}

export interface AppStatus {
  onboarded: boolean;
  scanning: boolean;
  analyzing: boolean;
  indexedFiles: number;
  lastScanAt: number | null;
  dataDir: string;
  holdingDir: string;
  home: string | null;
}

export interface ScanProgress {
  phase: string;
  root: string;
  files: number;
  dirs: number;
  bytes: number;
  current: string;
  elapsedMs: number;
  done: boolean;
  cancelled: boolean;
  error: string | null;
}

export interface AnalyzeProgress { phase: string; done: number; total: number }

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  defaultSettings: () => invoke<Settings>("default_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  appStatus: () => invoke<AppStatus>("app_status"),
  startScan: () => invoke<void>("start_scan"),
  cancelScan: () => invoke<void>("cancel_scan"),

  search: (query: string, limit = 50) => invoke<SearchResult>("search", { query, limit }),

  metrics: () => invoke<Metrics>("dashboard_metrics"),
  jumpBackIn: (limit = 8) => invoke<Session[]>("jump_back_in", { limit }),
  treemap: (path: string | null) => invoke<Treemap>("treemap", { path }),
  growth: (days = 90) => invoke<Growth>("growth_series", { days }),
  inbox: (limit = 500) => invoke<Inbox>("inbox_items", { limit }),
  inboxDismiss: (paths: string[]) => invoke<void>("inbox_dismiss", { paths }),
  archiveFiles: (paths: string[]) => invoke<ItemOutcome[]>("archive_files", { paths }),

  openFile: (path: string) => invoke<void>("open_file", { path }),
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
  fileDetails: (path: string) => invoke<FileDetails>("file_details", { path }),
  readText: (path: string, maxBytes?: number) =>
    invoke<{ text: string; truncated: boolean }>("read_text_preview", { path, maxBytes }),

  listTags: () => invoke<TagInfo[]>("list_tags"),
  addTag: (paths: string[], tag: string) => invoke<void>("add_tag", { paths, tag }),
  removeTag: (path: string, tag: string) => invoke<void>("remove_tag", { path, tag }),

  listSpaces: () => invoke<Space[]>("list_spaces"),
  saveSpace: (space: SpaceInput) => invoke<number>("save_space", { space }),
  deleteSpace: (id: number) => invoke<void>("delete_space", { id }),
  pinSpace: (id: number, pinned: boolean) => invoke<void>("pin_space", { id, pinned }),
  spaceFiles: (id: number, sort: string, limit = 2000, offset = 0) =>
    invoke<{ items: FileItem[]; total: number }>("space_files", { id, sort, limit, offset }),
  addToSpace: (id: number, paths: string[]) => invoke<void>("add_to_space", { id, paths }),
  removeFromSpace: (id: number, path: string) => invoke<void>("remove_from_space", { id, path }),

  timeline: (before: number | null, limit: number, filter: { kind?: string | null; spaceId?: number | null }) =>
    invoke<TimelineEntry[]>("timeline", { before, limit, filter }),

  places: () => invoke<Place[]>("places"),
  listDir: (path: string) => invoke<DirEntry[]>("list_dir", { path }),
  listSubdirs: (path: string) => invoke<{ name: string; path: string }[]>("list_subdirs", { path }),

  cleanupAnalyze: () => invoke<void>("cleanup_analyze"),
  cleanupCancel: () => invoke<void>("cleanup_cancel"),
  cleanupSummary: () => invoke<CleanupState>("cleanup_summary"),
  cleanupItems: (category: string) => invoke<Candidate[]>("cleanup_items", { category }),
  cleanupRun: (items: CleanItem[], dryRun: boolean) => invoke<RunReport>("cleanup_run", { items, dryRun }),
  moveToHolding: (paths: string[]) => invoke<RunReport>("move_to_holding", { paths }),
  holdingList: (includeDone = false) => invoke<HeldItem[]>("holding_list", { includeDone }),
  holdingRestore: (ids: number[]) => invoke<ItemOutcome[]>("holding_restore", { ids }),
  holdingPurgeExpired: () => invoke<number>("holding_purge_expired"),
  cleanupLog: (limit = 500) => invoke<LogEntry[]>("cleanup_log", { limit }),
};

export function assetUrl(path: string) {
  return convertFileSrc(path);
}

export function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload));
}
