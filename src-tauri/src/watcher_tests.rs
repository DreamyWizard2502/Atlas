//! Regression tests for the watcher stack overflow (unbounded recursion in
//! `apply_path` when a path's parent can't be indexed) and for how the watcher
//! handles very deep trees, large bursts, and Atlas's own writes.
//! Everything runs in generated temp directories.

use crate::db::Db;
use crate::indexer::{apply_path, Change};
use crate::settings::{Exclusions, Settings};
use crate::watcher::{self, OwnPaths};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Run `f` on a thread with a deliberately small stack. Anything whose stack use
/// grows with tree depth, or that recurses without bound, dies here instead of
/// passing by luck on a big default stack.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(512 * 1024).spawn(f).unwrap().join().unwrap()
}

/// Join with native separators (`Path::join("a/b")` keeps the '/').
fn j(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, seg| p.join(seg))
}

struct Fx {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    db: Db,
    excl: Exclusions,
    own: OwnPaths,
}

fn fixture() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let db = Db::open(tmp.path().join("atlas.db")).unwrap();
    let settings = Settings { roots: vec![root.to_string_lossy().to_string()], excluded: vec![], ..Default::default() };
    let excl = Exclusions::new(&settings, &[]);
    Fx { _tmp: tmp, root, db, excl, own: OwnPaths::default() }
}

fn scan(fx: &Fx) {
    crate::indexer::scan(&fx.db, &[fx.root.clone()], &fx.excl, &AtomicBool::new(false), |_| {}).unwrap();
}

fn count_under(db: &Db, dir: &Path) -> i64 {
    let (lo, hi) = crate::db::subtree_range(&dir.to_string_lossy());
    db.conn()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM files WHERE path >= ?1 AND path < ?2", [lo, hi], |r| r.get(0))
        .unwrap()
}

fn root_totals(db: &Db, root: &Path) -> (i64, i64) {
    db.conn()
        .unwrap()
        .query_row("SELECT size, file_count FROM files WHERE path = ?1", [root.to_string_lossy()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
}

fn process(fx: &Fx, paths: Vec<PathBuf>) -> usize {
    let (db, roots, excl, own) = (fx.db.clone(), vec![fx.root.clone()], fx.excl.clone(), fx.own.clone());
    on_small_stack(move || watcher::process_batch(&db, &roots, &excl, &own, &paths).unwrap())
}

// ---------------------------------------------------------------------------
// The crash: a parent that can't be indexed while its child can.

/// Exact shape of `C:\ProgramData\Microsoft\Windows\AppRepository\Packages`:
/// the folder itself can't be stat'ed or listed, but its children can be
/// stat'ed by full path (Windows grants traverse rights). The old code
/// recursed between child and parent forever.
#[cfg(windows)]
#[test]
fn unreadable_parent_does_not_recurse() {
    use std::process::Command;
    struct Acl(Vec<PathBuf>, String);
    impl Drop for Acl {
        fn drop(&mut self) {
            // Undo in reverse so each folder is reachable again before its child.
            for p in self.0.iter().rev() {
                let _ = Command::new("icacls").arg(p).args(["/remove:d", &self.1]).output();
            }
        }
    }
    let out = Command::new("whoami").args(["/user", "/fo", "csv", "/nh"]).output().unwrap();
    let csv = String::from_utf8_lossy(&out.stdout);
    let sid = csv.trim().rsplit(',').next().unwrap().trim_matches('"').to_string();
    let who = format!("*{sid}");

    let fx = fixture();
    let g = fx.root.join("G");
    let p = g.join("Packages");
    let file = j(&p, "Microsoft.AAD.BrokerPlugin/settings.dat");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"x").unwrap();

    let mut acl = Acl(vec![], who.clone());
    for (dir, perms) in [(&p, "(RD,RA,REA)"), (&g, "(RD)")] {
        let ok = Command::new("icacls").arg(dir).args(["/deny", &format!("{who}:{perms}")]).status().unwrap();
        assert!(ok.success());
        acl.0.push(dir.clone());
    }
    assert!(std::fs::symlink_metadata(&p).is_err(), "fixture: parent must be unreadable");
    assert!(std::fs::symlink_metadata(&file).is_ok(), "fixture: child must be reachable");

    scan(&fx);
    assert_eq!(count_under(&fx.db, &g), 0, "the scan can't see inside G either");

    // Events for the child (and its folder) must be dropped, not recursed on.
    let changed = process(&fx, vec![file.clone(), file.parent().unwrap().to_path_buf()]);
    assert_eq!(changed, 0);
    assert_eq!(count_under(&fx.db, &g), 0);
    drop(acl);
}

/// Same bug class through a junction / symlink: the scan skips links, so the
/// link has no row, yet a path "through" it exists on disk.
#[test]
fn link_parent_does_not_recurse() {
    let fx = fixture();
    let target = fx._tmp.path().join("outside");
    std::fs::create_dir_all(target.join("inner")).unwrap();
    std::fs::write(target.join("inner").join("f.txt"), b"hello").unwrap();
    let link = fx.root.join("link");
    #[cfg(windows)]
    {
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();

    scan(&fx);
    let through = j(&link, "inner/f.txt");
    assert!(std::fs::metadata(&through).is_ok());
    let conn = fx.db.conn().unwrap();
    let out = on_small_stack({
        let (db, root, excl) = (fx.db.clone(), fx.root.clone(), fx.excl.clone());
        move || apply_path(&db.conn().unwrap(), &through, &root, &excl).unwrap()
    });
    assert_eq!(out.change, Change::None);
    assert_eq!(process(&fx, vec![j(&link, "inner/f.txt"), link.join("inner")]), 0);
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "only the root row; nothing through the link");
}

// ---------------------------------------------------------------------------
// Deep trees and bursts.

const DEPTH: usize = 300;

fn deep_chain(base: &Path, depth: usize) -> PathBuf {
    let mut p = base.to_path_buf();
    for i in 0..depth {
        p = p.join(format!("d{i}"));
    }
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A 300-level tree appears and only its deepest file's event arrives (the
/// worst case: no ancestor events at all). The old code recursed once per
/// missing level, so stack use grew with depth.
#[test]
fn very_deep_tree_from_a_single_event() {
    let fx = fixture();
    scan(&fx);
    let bottom = deep_chain(&fx.root.join("deep"), DEPTH);
    let file = bottom.join("leaf.bin");
    std::fs::write(&file, vec![7u8; 4096]).unwrap();

    let changed = process(&fx, vec![file.clone()]);
    assert_eq!(changed, 1, "one new top-level folder, indexed with its whole subtree");
    assert_eq!(count_under(&fx.db, &fx.root.join("deep")), DEPTH as i64 + 1, "every level plus the file");
    assert_eq!(root_totals(&fx.db, &fx.root), (4096, 1));

    // Deleting the whole tree is reported from the top folder in one go.
    std::fs::remove_dir_all(fx.root.join("deep")).unwrap();
    assert_eq!(process(&fx, vec![file, fx.root.join("deep")]), 1);
    assert_eq!(count_under(&fx.db, &fx.root.join("deep")), 0);
    assert_eq!(root_totals(&fx.db, &fx.root), (0, 0));
}

/// Every level of a deep tree reported at once, in random order.
#[test]
fn very_deep_tree_with_every_event_shuffled() {
    let fx = fixture();
    scan(&fx);
    let bottom = deep_chain(&fx.root.join("deep"), DEPTH);
    std::fs::write(bottom.join("leaf.txt"), b"abc").unwrap();
    let mut paths: Vec<PathBuf> = bottom.ancestors().take(DEPTH + 1).map(Path::to_path_buf).collect();
    paths.push(bottom.join("leaf.txt"));
    shuffle(&mut paths);
    process(&fx, paths);
    assert_eq!(root_totals(&fx.db, &fx.root), (3, 1));
}

/// ~45k mixed changes in one batch (new files in new folders, edits, deletes),
/// shuffled. The result must equal what a fresh full scan produces.
#[test]
fn large_burst_matches_a_fresh_scan() {
    let fx = fixture();
    // Existing content: 20 folders x 250 files.
    for d in 0..20 {
        let dir = fx.root.join(format!("old{d}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..250 {
            std::fs::write(dir.join(format!("f{f}.txt")), b"old").unwrap();
        }
    }
    scan(&fx);

    let mut events = Vec::new();
    // New: 40 folders x 1000 files.
    for d in 0..40 {
        let dir = j(&fx.root, &format!("new/n{d}"));
        std::fs::create_dir_all(&dir).unwrap();
        events.push(dir.clone());
        for f in 0..1000 {
            let p = dir.join(format!("file_{f}.dat"));
            std::fs::write(&p, vec![1u8; f % 7]).unwrap();
            events.push(p);
        }
    }
    // Edits and deletes in the old content.
    for d in 0..20 {
        let dir = fx.root.join(format!("old{d}"));
        for f in 0..250 {
            let p = dir.join(format!("f{f}.txt"));
            match f % 3 {
                0 => std::fs::write(&p, b"edited, longer").unwrap(),
                1 => std::fs::remove_file(&p).unwrap(),
                _ => {}
            }
            events.push(p.clone());
            events.push(p); // duplicates happen in real bursts
        }
    }
    shuffle(&mut events);
    let n = events.len();
    let t = Instant::now();
    process(&fx, events);
    eprintln!("applied {n} events in {:?}", t.elapsed());

    let fresh = Db::open(fx._tmp.path().join("fresh.db")).unwrap();
    crate::indexer::scan(&fresh, &[fx.root.clone()], &fx.excl, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(root_totals(&fx.db, &fx.root), root_totals(&fresh, &fx.root));
    let rows = |db: &Db| -> i64 { db.conn().unwrap().query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap() };
    assert_eq!(rows(&fx.db), rows(&fresh));
    let deleted: i64 = fx
        .db
        .conn()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM events WHERE kind = 'deleted'", [], |r| r.get(0))
        .unwrap();
    let expected = 20 * (0..250).filter(|f| f % 3 == 1).count() as i64;
    assert_eq!(deleted, expected, "one timeline entry per deleted file");
}

fn shuffle<T>(v: &mut [T]) {
    // Small deterministic LCG; no extra dependency needed.
    let mut s: u64 = 0x2545_F491_4F6C_DD1D;
    for i in (1..v.len()).rev() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        v.swap(i, (s >> 33) as usize % (i + 1));
    }
}

// ---------------------------------------------------------------------------
// Atlas's own writes, end to end with the real watcher.

fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    f()
}

/// The database and holding areas live *inside* the watched root (as they do
/// when a whole drive is indexed) and the user has removed the AppData
/// exclusion. Atlas's own writes must never reach the index, and the watcher's
/// own DB writes must not trigger further rounds (no feedback loop).
#[test]
fn real_watcher_ignores_own_writes_without_feedback_loop() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("drive");
    let data_dir = j(&root, "Users/me/AppData/Local/app.atlas.files");
    let holding_primary = data_dir.join("holding");
    let holding_other = root.join(crate::cleanup::holding::HOLDING_DIR_NAME);
    for d in [&data_dir, &holding_primary, &holding_other] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::create_dir_all(root.join("docs")).unwrap();
    let db = Db::open(data_dir.join("atlas.db")).unwrap();
    let settings = Settings { roots: vec![root.to_string_lossy().to_string()], excluded: vec![], ..Default::default() };
    let own_dirs = vec![data_dir.clone(), holding_other.clone()];
    let excl = Exclusions::new(&settings, &own_dirs);
    crate::indexer::scan(&db, &[root.clone()], &excl, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(count_under(&db, &data_dir), 0);

    let batches = Arc::new(AtomicUsize::new(0));
    let overflows = Arc::new(AtomicUsize::new(0));
    let handle = {
        let (b, o) = (batches.clone(), overflows.clone());
        watcher::start(
            db.clone(),
            vec![root.clone()],
            excl.clone(),
            OwnPaths::new(&own_dirs),
            move |_| {
                b.fetch_add(1, Ordering::SeqCst);
            },
            move || {
                o.fetch_add(1, Ordering::SeqCst);
            },
        )
        .unwrap()
    };
    std::thread::sleep(Duration::from_millis(300));

    // Phase 1: only Atlas's own files change (DB, WAL, webview data, holding).
    let conn = db.conn().unwrap();
    for i in 0..200 {
        crate::db::kv_set(&conn, "noise", &i.to_string()).unwrap();
        std::fs::write(data_dir.join("EBWebView.log"), format!("{i}")).unwrap();
        std::fs::write(holding_primary.join(format!("held{}.bin", i % 10)), b"h").unwrap();
        std::fs::write(holding_other.join(format!("held{}.bin", i % 10)), b"h").unwrap();
    }
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(batches.load(Ordering::SeqCst), 0, "own writes must not produce index changes");
    assert_eq!(count_under(&db, &data_dir), 0);
    assert_eq!(count_under(&db, &holding_other), 0);

    // Phase 2: real user changes, including a deep new tree, are picked up.
    for i in 0..50 {
        std::fs::write(root.join("docs").join(format!("note{i}.txt")), b"hello").unwrap();
    }
    let leaf = deep_chain(&root.join("projects"), 40).join("model.step");
    std::fs::write(&leaf, vec![0u8; 1000]).unwrap();
    let ok = wait_until(Duration::from_secs(20), || {
        count_under(&db, &root.join("docs")) == 50 && count_under(&db, &root.join("projects")) == 41
    });
    assert!(ok, "docs={} projects={}", count_under(&db, &root.join("docs")), count_under(&db, &root.join("projects")));
    assert_eq!(root_totals(&db, &root).1, 51);

    // Phase 3: quiet. The watcher's own DB writes from phase 2 are inside the
    // watched root; they must not cause another round.
    std::thread::sleep(Duration::from_secs(2));
    let settled = batches.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(batches.load(Ordering::SeqCst), settled, "feedback loop: watcher reacted to its own DB writes");
    assert_eq!(count_under(&db, &data_dir), 0);
    assert_eq!(overflows.load(Ordering::SeqCst), 0);
    drop(handle);
}

#[test]
fn own_paths_match_short_names_and_holding_anywhere() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("app.atlas.files");
    std::fs::create_dir_all(&data).unwrap();
    let own = OwnPaths::new(&[data.clone()]);
    assert!(own.contains(&data.join("atlas.db-wal")));
    assert!(own.contains(&j(tmp.path(), "anything/.atlas-holding/b1/2/file.txt")));
    assert!(!own.contains(&tmp.path().join("app.atlas.files.bak")));
    // When the volume generates 8.3 names, the short form is recognised too.
    if let Some(short) = crate::platform::short_path(&data) {
        assert!(own.contains(&short.join("atlas.db")), "{}", short.display());
    }
}

/// End to end: a 30k-file burst through the real OS watcher must all land in
/// the index, and the watcher must still be alive afterwards. Slow (~17 s), so
/// opt-in: `cargo test -- --ignored real_watcher_large_burst`.
#[test]
#[ignore]
fn real_watcher_large_burst() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("r");
    std::fs::create_dir_all(root.join("burst")).unwrap();
    let db = Db::open(tmp.path().join("a.db")).unwrap();
    let settings = Settings { roots: vec![root.to_string_lossy().to_string()], excluded: vec![], ..Default::default() };
    let excl = Exclusions::new(&settings, &[]);
    crate::indexer::scan(&db, &[root.clone()], &excl, &AtomicBool::new(false), |_| {}).unwrap();
    let _h = watcher::start(db.clone(), vec![root.clone()], excl, OwnPaths::default(), |_| {}, || {}).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    for i in 0..30_000 {
        std::fs::write(root.join("burst").join(format!("file_with_a_longish_name_{i}.txt")), b"x").unwrap();
    }
    let converged = wait_until(Duration::from_secs(90), || count_under(&db, &root.join("burst")) == 30_000);
    assert!(converged, "indexed {} of 30000", count_under(&db, &root.join("burst")));
    std::fs::write(root.join("after.txt"), b"later").unwrap();
    let alive = wait_until(Duration::from_secs(15), || {
        db.conn().unwrap().query_row("SELECT COUNT(*) FROM files WHERE name = 'after.txt'", [], |r| r.get::<_, i64>(0)).unwrap() == 1
    });
    assert!(alive, "watcher stopped reporting after the burst");
}
