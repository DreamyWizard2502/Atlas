# Atlas — implementation plan

Atlas is a Windows-first desktop file manager whose home screen is a dashboard.
You navigate by intent (what am I working on, what changed, what is cluttering the
drive) rather than by path. The folder tree is still there, as a secondary view.

## 1. Architecture

```
┌──────────────────────────── WebView2 (React + TS + Tailwind) ───────────────────────────┐
│ Icon rail: Dashboard · Spaces · Timeline · Cleanup · Folders · Settings                  │
│ Global: Ctrl+K palette · Space = Quick Look · context menu · selection store            │
│ src/lib/api.ts  ── typed wrappers around invoke() + listen()                            │
│ src/theme/      ── 20 palettes as CSS variables; mode + light/dark picks; AA tests       │
└──────────────▲───────────────────────────────────────────────────────▲──────────────────┘
               │ Tauri commands (JSON)                                  │ events
┌──────────────┴───────────────────────── Rust (src-tauri) ─────────────┴─────────────────┐
│ commands.rs    thin #[tauri::command] layer; heavy work runs in spawn_blocking          │
│ platform/      OS abstraction (known folders, disk space, attributes, lock check,       │
│                installed apps, system roots). windows.rs now; other OSes later.         │
│ db.rs          SQLite (rusqlite, bundled, WAL), schema + migrations, FTS5               │
│ indexer.rs     jwalk parallel scan → channel → batched transactions (5k rows)           │
│ watcher.rs     notify watcher → 1 s batches → incremental upserts + change events       │
│ snapshots.rs   daily size-per-top-folder and size-per-type rows                         │
│ search.rs      query parser (type:, ext:, size:, modified:, in:) → FTS5 + SQL           │
│ dashboard.rs   metrics, jump back in (sessions), treemap, growth, inbox                 │
│ spaces.rs      rule → SQL compiler, tags, explicit members                              │
│ timeline.rs    day-grouped change feed                                                  │
│ folders.rs     directory listing for the classic view                                   │
│ preview.rs     Quick Look metadata + text reads                                         │
│ cleanup/       rules.rs · blocklist.rs · duplicates.rs · holding.rs · analyze.rs        │
│ settings.rs    typed settings stored as JSON in SQLite                                  │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

Principles
- The frontend never touches the disk. Everything goes through commands. Media
  preview uses Tauri's asset protocol (read-only) for images/video/audio/PDF.
- Search, dashboard, spaces and timeline query SQLite only. Only Cleanup's
  temp/cache category and the Folders view read the disk directly, and only in
  narrowly scoped directories.
- All OS-specific calls live in `platform/`, behind plain functions with a
  `#[cfg(windows)]` implementation and a portable fallback.
- Every command is `async` and pushes work to a blocking thread, so the UI thread
  is never blocked. Each command opens its own SQLite connection (WAL mode,
  busy timeout), so reads never wait behind the indexer's write batches.

## 2. Database schema (SQLite, WAL)

```sql
files(
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,      -- absolute path as reported by the OS
  parent TEXT NOT NULL,
  name TEXT NOT NULL,
  ext TEXT NOT NULL DEFAULT '',   -- lowercase, no dot
  is_dir INTEGER NOT NULL,
  size INTEGER NOT NULL DEFAULT 0, -- files: bytes; dirs: recursive total
  file_count INTEGER NOT NULL DEFAULT 0, -- dirs: recursive file count
  created INTEGER, modified INTEGER, accessed INTEGER, -- unix seconds
  attrs INTEGER NOT NULL DEFAULT 0, -- platform attribute bits (hidden/system)
  root TEXT NOT NULL,             -- indexed location this row belongs to
  gen INTEGER NOT NULL DEFAULT 0, -- scan generation (mark & sweep on rescan)
  hash TEXT, hash_key TEXT        -- blake3 cache, valid while hash_key = size:mtime
)
files_fts  -- FTS5 external-content table over files(name), unicode61, prefix 2/3,
           -- kept in sync with triggers
snapshots(day TEXT, kind TEXT /*total|folder|type*/, key TEXT, size INTEGER,
          count INTEGER, PRIMARY KEY(day, kind, key))
events(id, ts, kind /*deleted*/, path, name, ext, size)
                                     -- deletions seen by the watcher (timeline);
                                     -- creations/edits come from files.modified
activity(path PRIMARY KEY, opened_at)  -- files opened through Atlas
tags(id, name UNIQUE), file_tags(path, tag_id, PRIMARY KEY(path, tag_id))
spaces(id, name UNIQUE, icon, rules_json, pinned, created_at)
space_items(space_id, path, PRIMARY KEY(space_id, path)) -- explicit "add to space"
holding(id, batch_id, original_path, held_path, size, category, is_dir,
        held_at, expires_at, status /*held|restored|trashed|missing*/)
cleanup_log(id, ts, batch_id, action, path, detail, dry_run)
kv(key PRIMARY KEY, value)  -- settings JSON, cached cleanup summary, meta
```

## 3. Tauri commands

| Area | Commands |
|---|---|
| Setup / settings | `get_settings`, `save_settings`, `default_locations`, `app_status` |
| Indexer | `start_scan`, `cancel_scan`, `index_stats` · events `scan://progress`, `scan://done`, `index://changed` |
| Search | `search(query, limit)`, `palette_suggestions` |
| Dashboard | `dashboard_metrics`, `jump_back_in`, `treemap(path?)`, `growth_series(days)`, `inbox_items` |
| Files | `open_file`, `reveal_file`, `file_info`, `read_text_preview`, `move_to_holding` |
| Tags | `list_tags`, `add_tag`, `remove_tag`, `file_tags` |
| Spaces | `list_spaces`, `save_space`, `delete_space`, `space_files(id, sort)`, `add_to_space`, `remove_from_space`, `pin_space` |
| Timeline | `timeline(before_ts, limit, filters)` |
| Folders | `list_roots`, `list_dir(path)`, `list_subdirs(path)` |
| Cleanup | `cleanup_analyze` (event `cleanup://progress`), `cleanup_summary`, `cleanup_items(category)`, `cleanup_run(items, dry_run)`, `holding_list`, `holding_restore(ids)`, `holding_purge_expired`, `cleanup_log(limit)` |

## 4. Cleanup engine — safety model

1. Analysis never mutates anything. It produces candidate items per category:
   duplicates, large & cold, temp & caches, app junk (config rules), old
   installers, empty folders.
2. Duplicates: group index rows by size (≥ configurable minimum), then a 64 KiB
   partial blake3 hash, then a full hash only for partial-hash collisions. Hashes
   are cached in the `files` row keyed by size+mtime. Nothing is pre-selected.
3. Execution re-validates each item immediately before acting: exists, size
   unchanged, not blocklisted, not system/hidden-system, not locked, and for
   duplicates at least one copy of the group is left untouched.
4. "Clean" = `rename` into a holding area on the same volume
   (`%LOCALAPPDATA%\Atlas\holding` on the system volume, `X:\.atlas-holding`
   elsewhere). A cross-volume move is refused, never copied+deleted. Original
   path stored in `holding`.
5. After the retention period (default 30 days) held items go to the Recycle Bin
   via the `trash` crate. Nothing is ever hard-deleted.
6. Restore renames back to the original path (re-creating parents; never
   overwrites — a conflicting name gets a " (restored)" suffix).
7. Dry run executes the full validation path and logs what would happen.
8. Blocklist: `%SystemRoot%`, Program Files (both), ProgramData, AppData\Roaming
   (except known cache dirs), any `.git` component, the holding area, locked
   files, system / hidden+system files. `%SystemRoot%\Temp` is the one explicit
   exception (known temp dir).

## 5. Build order

1. Scaffold Tauri 2 + Vite + React + TS + Tailwind; Rust module skeleton.
2. Indexer + DB (schema, batched writer, progress events, dir size rollup,
   watcher, snapshots).
3. Search (parser + FTS5), Ctrl+K palette.
4. Dashboard widgets (metrics, jump back in, treemap, growth, inbox, cleanup
   card, pinned spaces).
5. Quick Look overlay (+ context menu, selection model).
6. Spaces (rules, tags, explicit members).
7. Cleanup engine with tests, cleanup page, holding area, log.
8. Timeline.
9. Folders view (tabs, dropdown breadcrumbs, details pane).
10. Settings page.
11. README, compile + test verification.

## 6. Tests (Rust, temp directories only)

- duplicate detection (size grouping, partial/full hash, distinct same-size files)
- cleanup rule matching (glob patterns, case-insensitivity, path patterns, min age)
- blocklist enforcement (system roots, Roaming exceptions, .git, holding area,
  Windows\Temp exception)
- holding area move + restore round trip (file and folder, name conflicts,
  duplicate-group guard, dry run leaves disk untouched)
- search query parser
