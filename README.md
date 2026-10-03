# Atlas

A calm, dashboard-first file manager for Windows. Instead of starting from a folder
tree, Atlas starts from intent: *what am I working on, what changed, and what's
cluttering my drive.* The classic folder view is still there, one click away.

Built with Tauri 2 (Rust) + React + TypeScript + Vite + Tailwind, with a SQLite
index (FTS5) owned entirely by the Rust backend. See [PLAN.md](PLAN.md) for the
architecture, DB schema, command list and build order.

## Features

- **Dashboard**: global search (Ctrl+K anywhere), drive / growth / inbox /
  reclaimable metrics, *Jump back in* (recent files grouped into work sessions),
  zoomable storage treemap, 90-day growth chart with the fastest-growing folder,
  a Downloads/Desktop inbox (flags installers that look already installed, and
  `file (2).pdf`-style duplicates), a cleanup summary and pinned spaces.
- **Search**: instant, index-only. Filters: `type:pdf`, `ext:sldprt,step`,
  `size:>500mb`, `modified:today | yesterday | this week | last week | this month | last month | 30d`,
  `in:"Space name"`, `is:folder`.
- **Spaces**: smart collections from rules (name contains, extension, type,
  folder, tag, date range, size), plus files added by hand. Files never move.
- **Timeline**: every change, day by day, filterable by type and space.
- **Cleanup**: duplicates, large and cold files, temp and caches, app junk
  (editable rule list), old installers, empty folders. Review lists, dry run,
  holding area with one-click restore, full action log.
- **Folders**: tree, tabs, breadcrumbs whose segments drop down to sibling
  folders, sortable list, details pane.
- **Everywhere**: Space = Quick Look (images, PDF, text/code, video, audio,
  metadata fallback). Esc closes and ←/→ steps through the list. Double-click opens
  with the default app. Right-click: open, reveal in Explorer, copy path, add tag,
  add to space, move to holding area.

## Cleanup safety model

- Nothing is ever permanently deleted. "Clean" **moves** items into a holding
  area on the **same drive** (`%LOCALAPPDATA%\app.atlas.files\holding` on the
  system drive, `X:\.atlas-holding` on others), so it's a rename, never a copy.
  A move that would cross drives is refused.
- The original path is recorded in SQLite. Restore puts items back, and never
  overwrites. A conflicting name is restored as `name (restored).ext`.
- After the retention period (default 30 days) held items go to the **Windows
  Recycle Bin** via the `trash` crate.
- Every item is re-validated right before it's moved: it must still exist, have
  the same size it had at review time, not be locked or open in another app, not
  be a system file, and not be blocklisted. The blocklist covers `C:\Windows`
  (except `C:\Windows\Temp`), Program Files (both), ProgramData,
  `AppData\Roaming` (except known cache folders), any `.git` folder, drive roots,
  and Atlas's own data.
- Duplicates are never pre-selected. You pick the copy to keep, and the backend
  refuses to clean every copy in a group.
- Dry-run mode runs the exact same validation and logs the result without
  moving anything. Every hold, restore, skip, error and Recycle Bin hand-off is
  logged (Cleanup → Log).

## Setup

Prerequisites (Windows 10/11, x64):

1. **Microsoft C++ Build Tools**: install "Visual Studio Build Tools" with the
   **Desktop development with C++** workload (MSVC v143+ and a Windows 10/11 SDK).
2. **Rust** (stable, MSVC toolchain) via <https://rustup.rs>:
   `rustup default stable-x86_64-pc-windows-msvc`
3. **Node.js** 20 or newer (tested with Node 24, npm 11).
4. **WebView2 runtime**: preinstalled on Windows 11. On Windows 10, install
   the Evergreen runtime from Microsoft if it's missing. The installer built
   below also bootstraps it.

Then:

```powershell
git clone <this repo> atlas
cd atlas
npm install
```

## Run in development

```powershell
npm run tauri dev
```

This starts Vite on `http://localhost:1420` and launches the app with hot reload.
On first launch Atlas asks which folders or drives to index (default: your user
folder) and indexes them in the background.

## Tests

```powershell
cd src-tauri
cargo test
```

The Rust tests cover duplicate detection, cleanup rule matching, blocklist
enforcement, the holding-area move/restore round trip (including dry run, name
conflicts, the keep-one-copy guard and retention), the indexer and its
incremental updates, the search parser, spaces, and an end-to-end smoke test
that runs every dashboard/timeline/folder query. All tests build their own
temporary directories and never touch real user folders. The Recycle Bin step
is injected as a stub in tests.

Optional scale benchmark (generates a synthetic tree in a temp dir):

```powershell
$env:ATLAS_BENCH_FILES = "200000"
cargo test --release --lib -- --ignored bench_scan --nocapture
```

On the development machine, 200,000 files across 20,501 folders indexed in about 8 s,
and a filename search took about 15 ms.

Frontend type check: `npm run typecheck`.

## Build an installer

```powershell
npm run tauri build
```

Outputs:

- `src-tauri\target\release\bundle\nsis\Atlas_0.1.0_x64-setup.exe`
- `src-tauri\target\release\bundle\msi\Atlas_0.1.0_x64_en-US.msi`

The first build downloads the NSIS and WiX tooling, so it needs network access.
The installers are not code-signed, so Windows SmartScreen will warn on install.

## Where data lives

- Index, settings, spaces, tags, snapshots, logs: `%LOCALAPPDATA%\app.atlas.files\atlas.db`
- Holding area: `%LOCALAPPDATA%\app.atlas.files\holding` (system drive),
  `X:\.atlas-holding` (other drives)

Both are always excluded from indexing and cleanup.

## Known limitations

- **Drag-out to other apps is not implemented.** Tauri 2 has no built-in API
  for dragging files out of the webview. It would need a third-party plugin
  (e.g. `tauri-plugin-drag`), which was left out for now.
- **"Already installed" is a heuristic.** Windows doesn't record which
  installer file was run, so Atlas matches the installer's name against
  installed programs in the registry's Uninstall keys.
- **Jump back in uses modified time plus files opened through Atlas.**
  Last-access times are unreliable on Windows, and files opened from other apps
  only show up once modified.
- **Tags and manually added space members are keyed by path.** Renaming or
  moving a file outside Atlas drops them.
- **Timeline history before the first scan comes from modification times.**
  Deletions are only recorded while Atlas is running. Copied files keep their
  original modification date.
- **Startup does a full rescan** (mark-and-sweep against the index) to catch
  changes made while Atlas was closed. It runs in the background, but it's a full walk.
- **Analysis item lists live in memory.** After a restart the dashboard still
  shows the last reclaimable total, but you need to re-run the analysis to review
  items. Each category's review list is capped at 20,000 rows.
- **Recycle Bin restores land in the holding folder.** Restoring a file from
  the Recycle Bin after retention puts it back where it was held, not at its
  original path (that information stays in Atlas's log).
- **`C:\Windows\Temp` usually needs admin rights.** Those items are skipped and
  logged.
- **Locked-file detection is conservative.** Any file another process has open
  is skipped.
- **Previews are limited to what WebView2 can render.** No HEIC/RAW/PSD/CAD
  thumbnails, no MKV/AVI playback, and Office documents show a metadata card.
  Windows Shell thumbnails aren't used.
- **The asset protocol scope is `**`.** It's read-only and only used for
  Quick Look and thumbnails, but it does let the webview read any file your user
  account can read.
- **Mid-word search uses a slower fallback.** Search is word-prefix based (FTS5).
  Mid-word matches (`voice` → `invoice`) use a slower substring scan, and only
  when there are no word matches.
- **macOS is not supported yet.** OS calls are isolated in
  `src-tauri/src/platform/`, but only the Windows backend is implemented. The
  portable fallback is a stub.
- **The holding folder on non-system drives (`X:\.atlas-holding`) is not marked
  hidden.**
