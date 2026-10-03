//! Read-only analysis producing review candidates per category.

use super::blocklist::Blocklist;
use super::duplicates::{self, DupInput};
use super::holding::dir_has_files;
use super::rules::CompiledRules;
use super::{Candidate, CategorySummary, CleanupSummary, CATEGORIES};
use crate::db::{now, subtree_range};
use crate::platform;
use crate::settings::Settings;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Upper bound on review rows per category (keeps the UI and memory sane).
const MAX_ITEMS: usize = 20_000;
const INSTALLER_EXTS: &[&str] = &["exe", "msi", "msix", "appx", "msixbundle", "appinstaller"];

#[derive(Debug, Default, Clone)]
pub struct Analysis {
    pub summary: CleanupSummary,
    pub items: HashMap<String, Vec<Candidate>>,
    /// path → every member path of its duplicate group
    pub dup_groups: HashMap<String, Vec<String>>,
}

fn label(id: &str) -> String {
    CATEGORIES.iter().find(|(c, _)| *c == id).map(|(_, l)| l.to_string()).unwrap_or_else(|| id.to_string())
}

fn name_of(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// Guess whether an installer was already run by matching its name against
/// installed applications. Returns the matching app name.
pub fn likely_installed(file_name: &str, installed: &[String]) -> Option<String> {
    const NOISE: &[&str] = &[
        "setup", "installer", "install", "x64", "x86", "amd64", "arm64", "win", "win64", "win32", "windows", "full",
        "latest", "release", "stable", "user", "system", "offline", "online", "web", "bootstrapper", "msi", "exe", "the",
    ];
    let stem = Path::new(file_name).file_stem()?.to_string_lossy().to_string();
    // Split on non-letters and camelCase boundaries.
    // "VSCodeUserSetup" → VS, Code, User, Setup.
    let chars: Vec<char> = stem.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphabetic() {
            if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if c.is_uppercase() && !cur.is_empty() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).map(|n| n.is_lowercase()).unwrap_or(false);
            if prev.is_lowercase() || (prev.is_uppercase() && next_lower) {
                words.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    let key = words
        .into_iter()
        .map(|w| w.to_lowercase())
        .find(|w| w.len() >= 3 && !NOISE.contains(&w.as_str()))?;
    installed
        .iter()
        .find(|name| name.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w == key))
        .cloned()
}

/// Machine-specific inputs, injectable so tests never read real user folders.
#[derive(Debug, Clone, Default)]
pub struct AnalyzeEnv {
    pub temp_dirs: Vec<PathBuf>,
    pub downloads: Option<PathBuf>,
    pub installed_apps: Vec<String>,
    pub protected: Vec<PathBuf>,
}

impl AnalyzeEnv {
    pub fn system() -> Self {
        let mut temp_dirs = vec![platform::temp_dir()];
        temp_dirs.extend(platform::system_temp_dir());
        temp_dirs.extend(platform::known_cache_dirs());
        AnalyzeEnv {
            temp_dirs,
            downloads: platform::downloads_dir(),
            installed_apps: platform::installed_app_names(),
            protected: platform::protected_user_folders(),
        }
    }
}

pub fn run(
    conn: &Connection,
    settings: &Settings,
    env: &AnalyzeEnv,
    blocklist: &Blocklist,
    cancel: &AtomicBool,
    mut progress: impl FnMut(&str, usize, usize),
) -> Result<Analysis> {
    let t = now();
    let mut items: HashMap<String, Vec<Candidate>> = HashMap::new();
    let mut truncated: HashSet<String> = HashSet::new();
    let ok = |p: &str| blocklist.check_path(Path::new(p)).is_ok();
    let mut push = |cat: &str, c: Candidate, items: &mut HashMap<String, Vec<Candidate>>| {
        let v = items.entry(cat.to_string()).or_default();
        if v.len() < MAX_ITEMS {
            v.push(c);
        } else {
            truncated.insert(cat.to_string());
        }
    };

    // ---- Large and cold ----
    progress("Large and cold files", 0, 0);
    {
        let min = settings.cold_min_mb as i64 * 1024 * 1024;
        let before = t - settings.cold_days as i64 * 86_400;
        let mut stmt = conn.prepare(
            "SELECT path, size, modified, attrs FROM files
             WHERE is_dir = 0 AND size >= ?1 AND COALESCE(modified, 0) < ?2 ORDER BY size DESC",
        )?;
        let rows = stmt.query_map(params![min, before], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, u32>(3)?))
        })?;
        for row in rows {
            let (path, size, modified, attrs) = row?;
            if platform::is_system(attrs) || attrs & platform::ATTR_CLOUD != 0 || !ok(&path) {
                continue;
            }
            let days = modified.map(|m| (t - m) / 86_400).unwrap_or(0);
            push(
                "large_cold",
                Candidate {
                    name: name_of(&path),
                    path,
                    size,
                    category: "large_cold".into(),
                    reason: format!("Not modified for {days} days"),
                    modified,
                    is_dir: false,
                    group: None,
                },
                &mut items,
            );
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(Analysis::default());
    }

    // ---- App-generated junk (rule list) ----
    progress("App-generated junk", 0, 0);
    {
        let compiled = CompiledRules::new(&settings.junk_rules);
        let mut stmt = conn.prepare("SELECT path, name, size, modified, attrs FROM files WHERE is_dir = 0")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let path: String = r.get(0)?;
            let name: String = r.get(1)?;
            let modified: Option<i64> = r.get(3)?;
            if let Some(rule) = compiled.find(&path, &name, modified, t) {
                let attrs: u32 = r.get(4)?;
                if platform::is_system(attrs) || !ok(&path) {
                    continue;
                }
                push(
                    "junk",
                    Candidate {
                        name,
                        path,
                        size: r.get(2)?,
                        category: "junk".into(),
                        reason: rule.name.clone(),
                        modified,
                        is_dir: false,
                        group: None,
                    },
                    &mut items,
                );
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(Analysis::default());
    }

    // ---- Old installers in Downloads ----
    progress("Old installers", 0, 0);
    if let Some(dl) = &env.downloads {
        let installed = &env.installed_apps;
        let (lo, hi) = subtree_range(&dl.to_string_lossy());
        let before = t - settings.inbox_days as i64 * 86_400;
        let ph = vec!["?"; INSTALLER_EXTS.len()].join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT path, name, size, modified, created FROM files
             WHERE is_dir = 0 AND path >= ? AND path < ? AND ext IN ({ph})
             AND MAX(COALESCE(created, 0), COALESCE(modified, 0)) < ? ORDER BY size DESC"
        ))?;
        let mut vals: Vec<rusqlite::types::Value> = vec![lo.into(), hi.into()];
        vals.extend(INSTALLER_EXTS.iter().map(|e| rusqlite::types::Value::Text(e.to_string())));
        vals.push(before.into());
        let rows = stmt.query_map(rusqlite::params_from_iter(vals), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<i64>>(3)?))
        })?;
        for row in rows {
            let (path, name, size, modified) = row?;
            if !ok(&path) {
                continue;
            }
            let reason = match likely_installed(&name, installed) {
                Some(app) => format!("Already installed: {app}"),
                None => format!("In Downloads for over {} days", settings.inbox_days),
            };
            push(
                "installers",
                Candidate { path, name, size, category: "installers".into(), reason, modified, is_dir: false, group: None },
                &mut items,
            );
        }
    }

    // ---- Empty folders ----
    progress("Empty folders", 0, 0);
    {
        let roots: HashSet<String> = settings.root_paths().iter().map(|r| platform::norm(r)).collect();
        let protected = &env.protected;
        let mut stmt = conn.prepare(
            "SELECT path, name, modified, attrs FROM files WHERE is_dir = 1 AND file_count = 0 ORDER BY path",
        )?;
        let rows: Vec<(String, String, Option<i64>, u32)> =
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        let mut taken: HashSet<String> = HashSet::new();
        for (path, name, modified, attrs) in rows {
            let p = Path::new(&path);
            let n = platform::norm(p);
            if roots.contains(&n)
                || name.starts_with('.')
                || attrs & (platform::ATTR_HIDDEN | platform::ATTR_SYSTEM) != 0
                || protected.iter().any(|x| platform::norm(x) == n)
                || !ok(&path)
                || p.ancestors().skip(1).any(|a| taken.contains(&platform::norm(a)))
            {
                continue;
            }
            // The index may exclude things (e.g. .git); confirm on disk.
            if !p.is_dir() || dir_has_files(p) {
                continue;
            }
            taken.insert(n);
            push(
                "empty_folders",
                Candidate {
                    path,
                    name,
                    size: 0,
                    category: "empty_folders".into(),
                    reason: "Contains no files".into(),
                    modified,
                    is_dir: true,
                    group: None,
                },
                &mut items,
            );
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(Analysis::default());
    }

    // ---- Temp and caches (walks only known safe directories) ----
    progress("Temp and caches", 0, 0);
    {
        let dirs = env.temp_dirs.clone();
        let mut seen_dirs: HashSet<String> = HashSet::new();
        let min_age = t - 86_400;
        for dir in dirs {
            if !seen_dirs.insert(platform::norm(&dir)) || !dir.is_dir() {
                continue;
            }
            for entry in jwalk::WalkDir::new(&dir).skip_hidden(false).follow_links(false).into_iter().flatten() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                let modified = crate::db::to_unix(meta.modified());
                if modified.map(|m| m > min_age).unwrap_or(true) {
                    continue;
                }
                if platform::is_system(platform::attrs(&meta)) || blocklist.check_path(&path).is_err() {
                    continue;
                }
                let path_s = path.to_string_lossy().to_string();
                push(
                    "temp",
                    Candidate {
                        name: name_of(&path_s),
                        path: path_s,
                        size: meta.len() as i64,
                        category: "temp".into(),
                        reason: format!("In {}", dir.display()),
                        modified,
                        is_dir: false,
                        group: None,
                    },
                    &mut items,
                );
            }
        }
    }

    // ---- Duplicates ----
    let mut dup_groups: HashMap<String, Vec<String>> = HashMap::new();
    {
        let min = settings.dup_min_kb as i64 * 1024;
        let mut stmt = conn.prepare(
            "SELECT path, size, modified, hash, hash_key, attrs FROM files
             WHERE is_dir = 0 AND size >= ?1 AND size IN
               (SELECT size FROM files WHERE is_dir = 0 AND size >= ?1 GROUP BY size HAVING COUNT(*) > 1)",
        )?;
        let inputs: Vec<DupInput> = stmt
            .query_map([min], |r| {
                Ok((
                    DupInput {
                        path: r.get(0)?,
                        size: r.get(1)?,
                        modified: r.get(2)?,
                        cached_hash: r.get(3)?,
                        cached_key: r.get(4)?,
                    },
                    r.get::<_, u32>(5)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .filter(|(i, attrs)| {
                attrs & platform::ATTR_CLOUD == 0 && !platform::is_system(*attrs) && blocklist.check_path(Path::new(&i.path)).is_ok()
            })
            .map(|(i, _)| i)
            .collect();
        let res = duplicates::find(inputs, cancel, |d, total| progress("Duplicates", d, total));
        if !res.new_hashes.is_empty() {
            let tx = conn.unchecked_transaction()?;
            {
                let mut up = tx.prepare("UPDATE files SET hash = ?1, hash_key = ?2 WHERE path = ?3")?;
                for (path, hash, key) in &res.new_hashes {
                    up.execute(params![hash, key, path])?;
                }
            }
            tx.commit()?;
        }
        for g in res.groups {
            let members: Vec<String> = g.files.iter().map(|f| f.path.clone()).collect();
            let n = members.len();
            for (i, f) in g.files.iter().enumerate() {
                dup_groups.insert(f.path.clone(), members.clone());
                push(
                    "duplicates",
                    Candidate {
                        name: name_of(&f.path),
                        path: f.path.clone(),
                        size: g.size,
                        category: "duplicates".into(),
                        reason: format!("Copy {} of {n}", i + 1),
                        modified: f.modified,
                        is_dir: false,
                        group: Some(g.id),
                    },
                    &mut items,
                );
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(Analysis::default());
    }

    // ---- Summary ----
    let mut categories = Vec::new();
    let mut counted: HashSet<String> = HashSet::new();
    let mut total = 0i64;
    for (id, _) in CATEGORIES {
        let list = items.get(*id).map(|v| v.as_slice()).unwrap_or(&[]);
        let reclaimable = if *id == "duplicates" {
            let mut by_group: HashMap<u32, (i64, i64)> = HashMap::new();
            for c in list {
                let e = by_group.entry(c.group.unwrap_or(0)).or_default();
                e.0 = c.size;
                e.1 += 1;
            }
            by_group.values().map(|(s, n)| s * (n - 1)).sum()
        } else {
            list.iter().map(|c| c.size).sum::<i64>()
        };
        let count = if *id == "duplicates" {
            list.iter().filter_map(|c| c.group).collect::<HashSet<_>>().len()
        } else {
            list.len()
        };
        if *id == "duplicates" {
            total += reclaimable;
        } else {
            for c in list {
                if counted.insert(c.path.clone()) {
                    total += c.size;
                }
            }
        }
        categories.push(CategorySummary {
            id: id.to_string(),
            label: label(id),
            count,
            reclaimable,
            truncated: truncated.contains(*id),
        });
    }
    Ok(Analysis {
        summary: CleanupSummary { analyzed_at: t, total_reclaimable: total, categories },
        items,
        dup_groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_guess() {
        let installed = vec!["Microsoft Visual Studio Code (User)".to_string(), "Google Chrome".to_string(), "7-Zip 24.08".to_string()];
        assert_eq!(likely_installed("ChromeSetup.exe", &installed).as_deref(), Some("Google Chrome"));
        assert_eq!(likely_installed("VSCodeUserSetup-x64-1.95.exe", &installed).as_deref(), Some("Microsoft Visual Studio Code (User)"));
        assert_eq!(likely_installed("Blender-4.2-windows-x64.msi", &installed), None);
        assert_eq!(likely_installed("setup.exe", &installed), None);
    }

    #[test]
    fn analysis_respects_blocklist_and_rules() {
        use crate::db::Db;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(200 * 86_400);
        let mk = |rel: &str, data: &[u8]| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, data).unwrap();
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(old).unwrap();
            p
        };
        mk("cad/~$Bracket.sldprt", b"lock");
        mk("cad/Bracket.sldprt", b"real part");
        mk("repo/.git/~$weird", b"never");
        mk("a/photo.jpg", &vec![9u8; 70_000]);
        mk("b/photo copy.jpg", &vec![9u8; 70_000]);
        mk("c/other.jpg", &vec![8u8; 70_000]);
        std::fs::create_dir_all(root.join("empty/nested")).unwrap();

        let db = Db::open(tmp.path().join("t.db")).unwrap();
        let mut settings = Settings { roots: vec![root.to_string_lossy().to_string()], excluded: vec![], ..Default::default() };
        settings.cold_min_mb = 0;
        let excl = crate::settings::Exclusions::new(&settings, &[]);
        crate::indexer::scan(&db, &[root.clone()], &excl, &AtomicBool::new(false), |_| {}).unwrap();
        let conn = db.conn().unwrap();
        let bl = Blocklist { system_roots: vec![tmp.path().join("Windows")], ..Default::default() };
        std::fs::create_dir_all(tmp.path().join("tempdir")).unwrap();
        let stale = tmp.path().join("tempdir").join("stale.tmp");
        std::fs::write(&stale, b"t").unwrap();
        std::fs::File::options().write(true).open(&stale).unwrap().set_modified(old).unwrap();
        std::fs::write(tmp.path().join("tempdir").join("fresh.tmp"), b"t").unwrap();
        let env = AnalyzeEnv { temp_dirs: vec![tmp.path().join("tempdir")], ..Default::default() };
        let a = run(&conn, &settings, &env, &bl, &AtomicBool::new(false), |_, _, _| {}).unwrap();

        let junk: Vec<&str> = a.items["junk"].iter().map(|c| c.name.as_str()).collect();
        assert_eq!(junk, vec!["~$Bracket.sldprt"]);
        let dups = &a.items["duplicates"];
        assert_eq!(dups.len(), 2);
        assert!(dups.iter().all(|c| c.name.starts_with("photo")));
        let dup_summary = a.summary.categories.iter().find(|c| c.id == "duplicates").unwrap();
        assert_eq!(dup_summary.reclaimable, 70_000);
        let empties: Vec<&str> = a.items["empty_folders"].iter().map(|c| c.name.as_str()).collect();
        assert_eq!(empties, vec!["empty"]);
        let temps: Vec<&str> = a.items["temp"].iter().map(|c| c.name.as_str()).collect();
        assert_eq!(temps, vec!["stale.tmp"]);
        // Nothing inside .git, ever.
        assert!(a.items.values().flatten().all(|c| !c.path.contains(".git")));
    }
}
