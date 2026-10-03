//! Activity exclusions: build output, caches, app data and machine-generated
//! files that would otherwise flood Jump Back In, the Timeline and the inbox.
//! They stay in the index (and searchable); they're only hidden from those
//! activity views. The list is editable in Settings.
//!
//! Entry syntax (case-insensitive):
//!   `target`          a folder name, anywhere in the path
//!   `~\.claude`       a path (file or folder); `~` and `%VAR%` are expanded
//!   `*.pdb`           a file-name pattern (`*` and `?`)
//!   `{id}.json`       a file named by a UUID, hash or number, with that ending
//!   `kind:object`     a kind confirmed from file contents (compiled object code)

use crate::cleanup::rules::glob_match_chars;
use anyhow::Result;
use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Arc;

pub fn default_activity_exclusions() -> Vec<String> {
    [
        // Build output and dependency folders.
        "target", "node_modules", ".git", "dist", "build", "obj", ".next", ".nuxt", ".svelte-kit", ".turbo",
        ".parcel-cache", ".cache", "__pycache__", ".pytest_cache", ".mypy_cache", ".gradle", ".vs", "CMakeFiles",
        ".fingerprint",
        // App data, tool state and temp folders.
        "~\\.claude", "~\\.claude.json", "~\\.cargo", "~\\.rustup", "~\\.npm", "~\\.vscode", "~\\AppData", "%TEMP%",
        // Compiler scratch, logs and machine-generated files.
        "kind:object", "*.o", "*.pdb", "*.ilk", "*.exp", "*.pch", "*.ipch", "*.tlog", "*.idb", "*.iobj", "*.ipdb",
        "*.rlib", "*.rmeta", "*.d", "*.timestamp", "*.lastbuildstate", "*.tmp", "*.log", "*.jsonl", "*.etl", "*.dmp",
        "{id}.json",
        // Shortcuts, settings and lock files that change on their own.
        "*.lnk", "*.ini", "*.db-journal", "*.db-wal", "*.db-shm", "*.lock", "*.lck", "*.crdownload", "*.part", "~$*",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Debug, Clone, Default)]
pub struct NoiseFilter {
    folders: Vec<String>,
    prefixes: Vec<String>,
    globs: Vec<Vec<char>>,
    ids: Vec<(String, String)>,
    kinds: Vec<String>,
    /// Indexed locations (lowercase). Folder-name rules only look below these,
    /// so indexing `D:\Build\Docs` doesn't hide everything because of `build`.
    roots: Vec<String>,
}

const SEP: char = std::path::MAIN_SEPARATOR;

impl NoiseFilter {
    /// Build from settings entries, expanding `~` and `%VAR%` for this user.
    pub fn new(entries: &[String]) -> Self {
        let home = crate::platform::home_dir();
        Self::with_env(entries, home.as_deref(), |v| std::env::var(v).ok())
    }

    pub fn with_env(entries: &[String], home: Option<&Path>, env: impl Fn(&str) -> Option<String>) -> Self {
        let mut f = NoiseFilter::default();
        for e in entries {
            let e = e.trim();
            if e.is_empty() || e.starts_with('#') {
                continue;
            }
            let lower = e.to_lowercase();
            if let Some(k) = lower.strip_prefix("kind:") {
                f.kinds.push(k.trim().to_string());
            } else if let Some((pre, suf)) = lower.split_once("{id}") {
                f.ids.push((pre.to_string(), suf.to_string()));
            } else if lower.contains('*') || lower.contains('?') {
                f.globs.push(lower.chars().collect());
            } else if lower.contains('\\') || lower.contains('/') || lower.starts_with('~') || lower.contains('%') || lower.contains(':') {
                if let Some(p) = expand(e, home, &env) {
                    f.prefixes.push(p);
                }
            } else {
                f.folders.push(lower);
            }
        }
        f
    }

    pub fn with_roots(mut self, roots: &[String]) -> Self {
        self.roots = roots.iter().map(|r| r.to_lowercase().replace('/', "\\").trim_end_matches(SEP).to_string()).collect();
        self.roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.prefixes.is_empty() && self.globs.is_empty() && self.ids.is_empty() && self.kinds.is_empty()
    }

    pub fn matches(&self, path: &str, name: &str, kind: Option<&str>) -> bool {
        if let Some(k) = kind {
            if self.kinds.iter().any(|x| x == k) {
                return true;
            }
        }
        let pl = path.to_lowercase().replace('/', "\\");
        if self.prefixes.iter().any(|p| pl == *p || (pl.starts_with(p.as_str()) && pl[p.len()..].starts_with(SEP))) {
            return true;
        }
        // Folder names: any folder below the indexed location, not the file itself.
        if !self.folders.is_empty() {
            let inner = self
                .roots
                .iter()
                .find(|r| pl.starts_with(r.as_str()) && pl[r.len()..].starts_with(SEP))
                .map(|r| &pl[r.len() + 1..])
                .unwrap_or(&pl);
            let mut parts: Vec<&str> = inner.split(SEP).collect();
            parts.pop();
            if parts.iter().any(|c| self.folders.iter().any(|f| f == c)) {
                return true;
            }
        }
        let nl = name.to_lowercase();
        if !self.globs.is_empty() {
            let chars: Vec<char> = nl.chars().collect();
            if self.globs.iter().any(|g| glob_match_chars(g, &chars)) {
                return true;
            }
        }
        self.ids.iter().any(|(pre, suf)| {
            nl.len() > pre.len() + suf.len()
                && nl.starts_with(pre.as_str())
                && nl.ends_with(suf.as_str())
                && is_machine_id(&nl[pre.len()..nl.len() - suf.len()])
        })
    }
}

/// UUIDs, long hex hashes, or plain numbers.
pub fn is_machine_id(s: &str) -> bool {
    let hex = |c: char| c.is_ascii_hexdigit();
    let uuid = s.len() == 36
        && s.char_indices().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { hex(c) });
    uuid || (s.len() >= 12 && s.chars().all(hex)) || (s.len() >= 3 && s.chars().all(|c| c.is_ascii_digit()))
}

fn expand(entry: &str, home: Option<&Path>, env: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut s = entry.replace('/', "\\");
    if let Some(rest) = s.strip_prefix('~') {
        s = format!("{}{}", home?.to_string_lossy(), rest);
    }
    // %VAR% expansion; an unknown variable drops the entry rather than guessing.
    while let Some(start) = s.find('%') {
        let end = start + 1 + s[start + 1..].find('%')?;
        let value = env(&s[start + 1..end])?;
        s.replace_range(start..=end, &value);
    }
    Some(s.trim_end_matches(SEP).to_lowercase())
}

/// Make `atlas_noise(path, name, kind)` available to SQL on this connection.
pub fn register(conn: &Connection, filter: Arc<NoiseFilter>) -> Result<()> {
    conn.create_scalar_function(
        "atlas_noise",
        3,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            let path: String = ctx.get(0)?;
            let name: String = ctx.get(1)?;
            let kind: Option<String> = ctx.get(2)?;
            Ok(filter.matches(&path, &name, kind.as_deref()) as i64)
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> NoiseFilter {
        let env = |v: &str| match v {
            "TEMP" => Some(r"C:\Users\me\AppData\Local\Temp".to_string()),
            _ => None,
        };
        NoiseFilter::with_env(&default_activity_exclusions(), Some(Path::new(r"C:\Users\me")), env)
    }

    fn hit(f: &NoiseFilter, path: &str, kind: Option<&str>) -> bool {
        f.matches(path, path.rsplit('\\').next().unwrap(), kind)
    }

    #[test]
    fn defaults_hide_build_output_app_data_and_scratch_files() {
        let f = defaults();
        // Real paths that flooded Jump Back In.
        for p in [
            r"C:\Users\me\Projects\atlas\src-tauri\target\release\deps\atlas_lib.atlas_lib.27806c-cgu.01.rcgu.o",
            r"C:\Users\me\Projects\atlas\src-tauri\target\release\.fingerprint\atlas-2a86\bin-atlas",
            r"C:\Users\me\Projects\atlas\src-tauri\target\release\libatlas_lib.d",
            r"C:\Users\me\Projects\web\node_modules\react\index.js",
            r"C:\Users\me\Projects\web\.git\index",
            r"C:\Users\me\Projects\web\dist\assets\index.js",
            r"C:\Users\me\.claude\projects\x\21f0af00-0b59-4f8f-b5f3-cab588b519c5.jsonl",
            r"C:\Users\me\.claude.json",
            r"C:\Users\me\AppData\Local\Temp\scratch.bin",
            r"C:\Users\me\Documents\export\45204.json",
            r"C:\Users\me\Documents\~$Report.docx",
        ] {
            assert!(hit(&f, p, None), "should be hidden: {p}");
        }
        // Compiled object code is hidden by content, wherever it lives.
        assert!(hit(&f, r"C:\Users\me\Desktop\flag_check.obj", Some("object")));
    }

    #[test]
    fn defaults_keep_real_work_visible() {
        let f = defaults();
        for (p, kind) in [
            (r"C:\Users\me\Documents\Quote 2026.pdf", Some("pdf")),
            (r"C:\Users\me\CAD\bracket v3.sldprt", Some("cad")),
            (r"C:\Users\me\CAD\scan.obj", Some("cad")), // a Wavefront model stays visible
            (r"C:\Users\me\Projects\atlas\src\App.tsx", Some("code")),
            (r"C:\Users\me\Projects\atlas\package.json", Some("code")),
            (r"C:\Users\me\Pictures\target practice.jpg", Some("image")),
            (r"C:\Users\me\Documents\builds of the year.docx", Some("doc")),
            (r"C:\Users\me\Documents\v12.json", Some("code")),
        ] {
            assert!(!hit(&f, p, kind), "should stay visible: {p}");
        }
    }

    #[test]
    fn entry_syntax() {
        let f = NoiseFilter::with_env(
            &["Renders".into(), r"D:\Scratch".into(), "*.bak".into(), "cache-{id}".into(), "kind:installer".into(), "%NOPE%\\x".into()],
            None,
            |_| None,
        );
        assert!(hit(&f, r"C:\work\Renders\frame1.png", None));
        assert!(!hit(&f, r"C:\work\Renders", None), "folder rules apply to what's inside");
        assert!(hit(&f, r"D:\Scratch", None) && hit(&f, r"D:\scratch\a\b.txt", None));
        assert!(!hit(&f, r"D:\ScratchPad\b.txt", None));
        assert!(hit(&f, r"C:\a\plan.BAK", None));
        assert!(hit(&f, r"C:\a\cache-0123456789abcdef", None) && !hit(&f, r"C:\a\cache-final", None));
        assert!(hit(&f, r"C:\a\setup.exe", Some("installer")));
        assert!(!f.is_empty());
        assert!(NoiseFilter::new(&[]).is_empty());
    }

    #[test]
    fn folder_rules_only_apply_inside_the_indexed_location() {
        let f = NoiseFilter::with_env(&["build".into(), "target".into()], None, |_| None).with_roots(&[r"D:\Build\Docs".into()]);
        assert!(!hit(&f, r"D:\Build\Docs\plan.pdf", None));
        assert!(hit(&f, r"D:\Build\Docs\site\build\index.html", None));
        // Without a matching root, every folder in the path counts.
        assert!(hit(&f, r"E:\target\x.txt", None));
    }

    #[test]
    fn machine_ids() {
        assert!(is_machine_id("21f0af00-0b59-4f8f-b5f3-cab588b519c5"));
        assert!(is_machine_id("ecf6c528cd2ffde6"));
        assert!(is_machine_id("45204"));
        assert!(!is_machine_id("v12"));
        assert!(!is_machine_id("deadbeef"));
        assert!(!is_machine_id("report"));
    }

    #[test]
    fn sql_function_filters_rows() {
        let conn = Connection::open_in_memory().unwrap();
        register(&conn, Arc::new(defaults())).unwrap();
        let hidden: i64 = conn
            .query_row("SELECT atlas_noise(?1, ?2, NULL)", [r"C:\Users\me\p\target\a.rlib", "a.rlib"], |r| r.get(0))
            .unwrap();
        let shown: i64 = conn
            .query_row("SELECT atlas_noise(?1, ?2, 'pdf')", [r"C:\Users\me\Documents\a.pdf", "a.pdf"], |r| r.get(0))
            .unwrap();
        assert_eq!((hidden, shown), (1, 0));
    }
}
