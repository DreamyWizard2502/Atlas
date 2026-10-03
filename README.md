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
  a storage treemap, a 90-day growth chart with the fastest-growing folder,
  a Downloads/Desktop inbox (flags installers that look already installed, and
  `file (2).pdf`-style duplicates), a cleanup summary and pinned spaces.
- **Storage treemap**: tiles are colored by their dominant content (Games and
  apps, CAD and 3D, Documents, Media, Code and projects, Archives and
  installers, Temp and cache, Other) by bytes, with a legend. Every tile is
  at least 30 px; smaller items fold into one "Other: N items" tile that opens
  a sorted list. Every tile has a tooltip (name, size, share, items,
  category). You can switch to a sorted list view, and click to zoom.
- **Drive growth**: on first launch the 90-day history is estimated from the
  creation dates of indexed files (drawn dashed and labelled), then real daily
  snapshots take over. The estimate is shifted so it meets the first real
  snapshot exactly.
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
- **Themes**: 20 palettes (10 dark, 10 light). Mode is Light, Dark or Match
  Windows, with separate light and dark palette picks, so Match Windows flips
  between your two choices. Pick them in Settings › Appearance; changes apply
  instantly and persist. Every palette meets WCAG AA for all text, and colors
  keep their meaning in every palette (amber = needs attention, green =
  reclaimable, red = destructive, gray = cold).
- **Activity views stay about your work**: Jump Back In, the Timeline and the
  inbox hide build output (`target`, `node_modules`, `dist`, `build`…),
  `.git`, `~\.claude` and other app data, `%TEMP%`, compiler scratch
  (`*.pdb`, `*.o`, `*.rlib`, `*.d`…), logs and `*.jsonl`, files named by a UUID
  or hash, and compiled object files detected by content. They stay indexed
  and searchable. The list is editable in Settings › Hidden from activity
  (`target` = folder name, `~\.claude` = path, `*.pdb` = pattern,
  `{id}.json` = machine-named file, `kind:object` = detected by content).
- **Everywhere**: Space = Quick Look (images, PDF, text/code, video, audio,
  metadata fallback). The preview type comes from the file's contents, never
  its extension alone. A `.png` that isn't a PNG, a truncated PDF, binary data
  in a `.txt`, an empty file, or a file that is locked or deleted mid-read gets
  a metadata card that explains why. Ambiguous extensions are sniffed: an MSVC
  `.obj` (COFF object code) is shown as a compiled object file, a Wavefront
  `.obj` as a 3D model. Each screen, dashboard card and overlay has its own
  error boundary, so one failing view never blanks the window. Esc closes and ←/→ steps through the list. Double-click opens
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
- Program and game installs are never offered in any category except temp and
  caches. That covers Program Files, WindowsApps, Steam and SteamLibrary, Epic,
  GOG, EA/Origin, Ubisoft, Battle.net, Riot, Xbox, Amazon, Rockstar and itch
  libraries, plus any folder holding an `.exe` next to its DLLs or packed
  assets (portable apps and DRM-free games).
- Large and cold files are listed under **Review or archive** with their own
  total. They are never counted as reclaimable, because old isn't the same as junk.
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
that runs every dashboard/timeline/folder query. Watcher regression tests cover
folders Windows won't let you read (the `AppRepository\Packages` crash), links,
a 300-level-deep tree, a 45k-event burst checked against a fresh scan, and an
end-to-end run proving the watcher ignores Atlas's own database and holding
areas with no feedback loop. The deep-tree and unreadable-folder tests run on a
512 KB stack so any recursion that grows with depth fails loudly. All tests build their own
temporary directories and never touch real user folders. The Recycle Bin step
is injected as a stub in tests.

Other Rust tests cover:
- content sniffing (MSVC vs Wavefront `.obj`)
- storage categories
- the activity exclusion rules and their SQL function
- Quick Look probing of hostile files (fake extensions, truncated PDFs,
  binary text, empty, vanished and locked files)
- install-folder exclusion and the review-or-archive split in cleanup
- the growth backfill and blend
- the v1 → v2 schema migration

Optional scale benchmark (generates a synthetic tree in a temp dir):

```powershell
$env:ATLAS_BENCH_FILES = "200000"
cargo test --release --lib -- --ignored bench_scan --nocapture
```

On the development machine, 200,000 files across 20,501 folders indexed in about 8 s,
and a filename search took about 15 ms.

Real OS watcher under a 30,000-file burst (about 20 s):

```powershell
cargo test --lib -- --ignored real_watcher_large_burst
```

Frontend tests use Node's built-in test runner. They cover palette contrast
for every text/background pair in all 20 palettes, semantic color checks,
treemap category colors (AA labels, distinct from each other and from the
meaning colors), theme mode resolution, and treemap layout (minimum tile size,
"Other" grouping):

```powershell
npm test
```

Palettes are generated by `scripts/gen_palettes.py`, which picks each palette's
character colors and adjusts only lightness until every pair passes AA.
Regenerate with `python scripts/gen_palettes.py src/theme/palettes.ts`, or edit
`src/theme/palettes.ts` by hand; `npm test` catches any contrast regression.

Frontend type check: `npm run typecheck`.

### Isolated profile for QA

Set `ATLAS_DATA_DIR` to run an instance with its own index, settings and
holding area, for example next to your normal instance. Also set
`WEBVIEW2_USER_DATA_FOLDER` so the two instances don't share a WebView2 profile:

```powershell
$env:ATLAS_DATA_DIR = "C:\scratch\atlas-qa"
$env:WEBVIEW2_USER_DATA_FOLDER = "C:\scratch\atlas-qa\webview"
.\src-tauri\target\release\atlas.exe
```

## Build an installer

```powershell
npm run tauri build
```

Outputs:

- `src-tauri\target\release\bundle\nsis\Atlas_0.1.0_x64-setup.exe`
- `src-tauri\target\release\bundle\msi\Atlas_0.1.0_x64_en-US.msi`

The first build downloads the NSIS and WiX tooling, so it needs network access.
The installers are not code-signed, so Windows SmartScreen will warn on install.

## Crash and error log

Atlas keeps a persistent log, so diagnosing a crash never depends on a terminal
being open. **Settings › Crash and error log › Open logs folder** opens it.

- **Rust panics:** message, location, thread and backtrace. Release builds
  include line tables (`[profile.release] debug = "line-tables-only"`), so
  backtraces name functions and lines on the machine that built them.
- **Frontend errors:** errors caught by any error boundary (with the component
  stack), plus uncaught JS errors and unhandled promise rejections. These are
  de-duplicated and capped at 50 per session.
- **Unclean exits:** native crashes such as a stack overflow never reach the
  panic hook, so each launch leaves a session marker. If the previous session
  didn't close cleanly, the next launch records that. Windows keeps the native
  details in Event Viewer › Windows Logs › Application.

The log is `logs\atlas.log` in Atlas's data folder. It rolls over at 1 MB and
keeps three older files (`atlas.1.log` … `atlas.3.log`).

## Where data lives

- Index, settings, spaces, tags, snapshots: `%LOCALAPPDATA%\app.atlas.files\atlas.db`
- Crash and error log: `%LOCALAPPDATA%\app.atlas.files\logs\atlas.log`
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
- **Folders your account can't read are invisible, live and at scan time.**
  Some system folders (for example `C:\ProgramData\Microsoft\Windows\AppRepository\Packages`)
  can't be listed. Changes inside them are ignored, the same as the full scan.
- **An extreme change burst can stop live updates until the next launch.**
  notify's Windows backend stops watching a folder, without reporting it, if
  the OS change buffer overflows. A 30,000-file burst didn't trigger this in
  testing. Bursts over 200,000 paths are turned into a rescan on purpose.
- **The growth backfill is an estimate.** It only knows about files that still
  exist, and copies are dated when they were copied. It's drawn dashed and
  replaced by real daily snapshots as they accumulate.
- **Treemap category colors have to share a narrow lightness band** to keep
  labels at AA. The closest pair in any palette is ΔE 6.8 (OKLab ×100): above
  the floor for colors backed by labels, a legend and tooltips, but short of
  the ideal 8.
- **Content sniffing is table-driven and covers `.obj` today.** Other
  misleading extensions are handled at preview time by the Quick Look probe.
- **The install-folder rule is a heuristic** (an `.exe` next to DLLs or packed
  assets). It errs toward protecting folders, so a folder holding a portable
  app is never offered for cleanup.
- **macOS is not supported yet.** OS calls are isolated in
  `src-tauri/src/platform/`, but only the Windows backend is implemented. The
  portable fallback is a stub.
- **The holding folder on non-system drives (`X:\.atlas-holding`) is not marked
  hidden.**
