//! End-to-end smoke test: scan a generated temp tree, then exercise every
//! read path the UI uses so SQL mistakes surface in `cargo test`, not at runtime.

use crate::db::Db;
use crate::settings::{Exclusions, Settings};
use crate::{dashboard, folders, preview, search, snapshots, spaces, timeline};
use rusqlite::params;
use std::sync::atomic::AtomicBool;

fn no_noise() -> std::sync::Arc<crate::noise::NoiseFilter> {
    std::sync::Arc::new(crate::noise::NoiseFilter::default())
}

/// Join with native separators (Path::join keeps a literal '/' inside the string).
fn j(base: &std::path::Path, rel: &str) -> std::path::PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, seg| p.join(seg))
}

#[test]
fn every_query_path_runs_against_a_real_index() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("home");
    let mk = |rel: &str, data: &[u8]| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
        p
    };
    mk("Projects/Bracket/bracket_v2.sldprt", &vec![1u8; 5000]);
    mk("Projects/Bracket/Invoice 2026-03.pdf", &vec![2u8; 3000]);
    mk("Projects/Bracket/notes.md", b"# Notes\nhello");
    mk("Photos/trip.jpg", &[&[0xFFu8, 0xD8, 0xFF, 0x00][..], &vec![3u8; 8996][..]].concat());
    mk("Photos/trip (2).jpg", &[&[0xFFu8, 0xD8, 0xFF, 0x00][..], &vec![3u8; 8996][..]].concat());

    let db = Db::open(tmp.path().join("atlas.db")).unwrap();
    let settings = Settings { roots: vec![root.to_string_lossy().to_string()], excluded: vec![], ..Default::default() };
    db.conn().unwrap();
    settings.save(&db.conn().unwrap()).unwrap();
    let excl = Exclusions::new(&settings, &[]);
    let p = crate::indexer::scan(&db, &[root.clone()], &excl, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(p.files, 5);
    let conn = db.conn().unwrap();

    // Search: text, filters, fallback substring, filter-only.
    let r = search::run(&conn, "bracket", 50).unwrap();
    assert!(r.items.iter().any(|f| f.name == "bracket_v2.sldprt"), "{:?}", r.items);
    let r = search::run(&conn, "type:pdf", 50).unwrap();
    assert_eq!(r.items.len(), 1);
    let r = search::run(&conn, "voice", 50).unwrap(); // mid-word → LIKE fallback
    assert_eq!(r.items.len(), 1);
    let r = search::run(&conn, "size:>4kb ext:jpg", 50).unwrap();
    assert_eq!(r.items.len(), 2);
    let r = search::run(&conn, "modified:today", 50).unwrap();
    assert!(r.items.len() >= 5);
    let r = search::run(&conn, "in:nope", 50).unwrap();
    assert!(r.unknown_space && r.items.is_empty());

    // Spaces + in: filter.
    let id = spaces::save(
        &conn,
        &spaces::SpaceInput {
            id: None,
            name: "Bracket work".into(),
            icon: None,
            pinned: Some(true),
            rules: spaces::SpaceRules {
                mode: "all".into(),
                rules: vec![spaces::Rule { field: "folder".into(), value: root.join("Projects").to_string_lossy().to_string() }],
            },
        },
    )
    .unwrap();
    assert_eq!(spaces::files(&conn, id, "recent", 50, 0).unwrap().total, 3);
    let r = search::run(&conn, "in:\"Bracket work\" type:pdf", 50).unwrap();
    assert_eq!(r.items.len(), 1);
    let listed = spaces::list(&conn).unwrap();
    assert_eq!(listed[0].count, 3);

    // Dashboard widgets.
    let m = dashboard::metrics(&conn, &settings).unwrap();
    assert_eq!(m.indexed_files, 5);
    assert_eq!(m.indexed_bytes, 5000 + 3000 + 13 + 9000 + 9000);
    conn.execute(
        "INSERT INTO activity(path, opened_at) VALUES (?1, ?2)",
        params![j(&root, "Photos/trip.jpg").to_string_lossy(), crate::db::now() + 5],
    )
    .unwrap();
    let sessions = dashboard::jump_back_in(&conn, 8, 45, &no_noise()).unwrap();
    assert_eq!(sessions.iter().map(|s| s.files.len()).sum::<usize>(), 5);
    assert!(sessions[0].files[0].opened);
    let tm = dashboard::treemap(&conn, None, 60).unwrap();
    assert_eq!(tm.children.len(), 2);
    assert_eq!(tm.children[0].name, "Photos");
    let sub = dashboard::treemap(&conn, Some(root.join("Projects").to_string_lossy().to_string()), 60).unwrap();
    assert_eq!(sub.crumbs.len(), 2);
    let tm_small = dashboard::treemap(&conn, Some(root.join("Photos").to_string_lossy().to_string()), 1).unwrap();
    assert!(tm_small.children.iter().any(|c| c.other));
    snapshots::take(&conn).unwrap();
    // Fake an older snapshot so growth + fastest folder have two points.
    conn.execute(
        "INSERT INTO snapshots(day, kind, key, size, count) VALUES ('2000-01-01', 'total', '', 100, 1),
         ('2000-01-01', 'folder', ?1, 10, 1)",
        [root.join("Photos").to_string_lossy()],
    )
    .unwrap();
    let g = dashboard::growth(&conn, 365 * 40).unwrap();
    assert!(g.points.len() >= 2);
    assert_eq!(g.fastest.as_ref().unwrap().name, "Photos");
    dashboard::inbox(&conn, &settings, 100, &no_noise()).unwrap();

    // Timeline (with and without filters, plus a deletion event).
    conn.execute(
        "INSERT INTO events(ts, kind, path, name, ext, size) VALUES (?1, 'deleted', 'C:\\x\\gone.pdf', 'gone.pdf', 'pdf', 0)",
        [crate::db::now()],
    )
    .unwrap();
    let t = timeline::page(&conn, None, 100, &timeline::TimelineFilter::default(), &no_noise()).unwrap();
    assert_eq!(t.len(), 6);
    let t = timeline::page(&conn, None, 100, &timeline::TimelineFilter { kind: Some("pdf".into()), space_id: None }, &no_noise()).unwrap();
    assert_eq!(t.len(), 2);
    let t = timeline::page(&conn, None, 100, &timeline::TimelineFilter { kind: None, space_id: Some(id) }, &no_noise()).unwrap();
    assert_eq!(t.len(), 3);
    let older = timeline::page(&conn, Some(t[0].ts), 100, &timeline::TimelineFilter::default(), &no_noise()).unwrap();
    assert!(older.len() <= 6);

    // Folders view + preview.
    let entries = folders::list_dir(&conn, &root.join("Projects").to_string_lossy()).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].size, Some(8013));
    assert_eq!(folders::list_subdirs(&root.to_string_lossy()).unwrap().len(), 2);
    folders::places(&settings);
    let d = preview::details(&conn, &j(&root, "Projects/Bracket/Invoice 2026-03.pdf").to_string_lossy()).unwrap();
    assert_eq!(d.spaces, vec!["Bracket work".to_string()]);
    let txt = preview::read_text(&j(&root, "Projects/Bracket/notes.md").to_string_lossy(), 1024).unwrap();
    assert!(txt.text.starts_with("# Notes"));
    assert!(preview::read_text(&j(&root, "Photos/trip.jpg").to_string_lossy(), 1024).is_err());
}

/// Scale check (opt-in): `ATLAS_BENCH_FILES=200000 cargo test --release -- --ignored bench_scan --nocapture`
#[test]
#[ignore]
fn bench_scan() {
    let n: usize = std::env::var("ATLAS_BENCH_FILES").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("bench");
    let t0 = std::time::Instant::now();
    for i in 0..n {
        let dir = root.join(format!("d{:03}", i % 500)).join(format!("s{:02}", (i / 500) % 40));
        if i < 500 * 40 {
            std::fs::create_dir_all(&dir).unwrap();
        }
        std::fs::write(dir.join(format!("file_{i}.txt")), b"x").unwrap();
    }
    eprintln!("generated {n} files in {:?}", t0.elapsed());
    let db = Db::open(tmp.path().join("atlas.db")).unwrap();
    let settings = Settings { roots: vec![root.to_string_lossy().to_string()], excluded: vec![], ..Default::default() };
    let excl = Exclusions::new(&settings, &[]);
    let mut events = 0;
    let mut max_gap = std::time::Duration::ZERO;
    let mut last = std::time::Instant::now();
    let t1 = std::time::Instant::now();
    let p = crate::indexer::scan(&db, &[root.clone()], &excl, &AtomicBool::new(false), |_| {
        events += 1;
        max_gap = max_gap.max(last.elapsed());
        last = std::time::Instant::now();
    })
    .unwrap();
    let scan_time = t1.elapsed();
    eprintln!("scanned {} files, {} dirs in {:?} ({} progress events, max gap {:?})", p.files, p.dirs, scan_time, events, max_gap);
    let conn = db.conn().unwrap();
    let t2 = std::time::Instant::now();
    let r = search::run(&conn, "file_12345", 50).unwrap();
    eprintln!("search 'file_12345': {} hits in {:?}", r.items.len(), t2.elapsed());
    let t3 = std::time::Instant::now();
    let r = search::run(&conn, "type:text modified:today", 50).unwrap();
    eprintln!("search filters: {} hits in {:?}", r.items.len(), t3.elapsed());
    let t4 = std::time::Instant::now();
    dashboard::treemap(&conn, None, 60).unwrap();
    dashboard::jump_back_in(&conn, 8, 45, &no_noise()).unwrap();
    eprintln!("treemap + jump back in: {:?}", t4.elapsed());
    assert_eq!(p.files as usize, n);
}

#[test]
fn activity_views_hide_noise_but_search_still_finds_it() {
    // Generated under target/ rather than %TEMP%: category and exclusion rules
    // deliberately treat anything inside a Temp folder as temp.
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-fixtures");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::tempdir_in(&base).unwrap();
    let root = tmp.path().join("home");
    let mk = |rel: &str, data: &[u8]| {
        let p = j(&root, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
    };
    let coff = [0x64u8, 0x86, 4, 0, 0xd8, 0xfd, 0x32, 0xef, 0xbe, 1, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, b'.', b'd', b'r'];
    mk("proj/target/release/deps/atlas_lib.rlib", &[1; 4000]);
    mk("proj/target/release/build/blake3-1/out/flag_check.obj", &coff);
    mk("Desktop/flag_check.obj", &coff); // a compiled object outside any build folder
    mk(".claude/projects/x/21f0af00-0b59-4f8f-b5f3-cab588b519c5.jsonl", b"{}\n");
    mk("Documents/report.pdf", b"%PDF-1.7 report");
    mk("CAD/cube.obj", b"o Cube\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
    mk("Media/clip.mp4", &[0; 9000]);

    let db = Db::open(tmp.path().join("atlas.db")).unwrap();
    let settings = Settings { roots: vec![root.to_string_lossy().to_string()], excluded: vec![], ..Default::default() };
    let excl = Exclusions::new(&settings, &[]);
    crate::indexer::scan(&db, &[root.clone()], &excl, &AtomicBool::new(false), |_| {}).unwrap();
    let conn = db.conn().unwrap();
    // The default list with `~` = this fixture's home. (The fixture itself lives
    // under the real %TEMP%, which the defaults hide, so point TEMP elsewhere.)
    let other_temp = tmp.path().join("elsewhere").to_string_lossy().to_string();
    let noise = std::sync::Arc::new(crate::noise::NoiseFilter::with_env(&settings.activity_exclusions, Some(&root), |v| {
        (v == "TEMP").then(|| other_temp.clone())
    })
    .with_roots(&settings.roots));

    let recent: Vec<String> = dashboard::jump_back_in(&conn, 20, 45, &noise)
        .unwrap()
        .into_iter()
        .flat_map(|s| s.files.into_iter().map(|f| f.file.name))
        .collect();
    let mut recent_sorted = recent.clone();
    recent_sorted.sort();
    assert_eq!(recent_sorted, vec!["clip.mp4", "cube.obj", "report.pdf"], "{recent:?}");
    let feed: Vec<String> = timeline::page(&conn, None, 100, &timeline::TimelineFilter::default(), &noise)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(feed.len(), 3, "{feed:?}");

    // Still searchable.
    assert!(!search::run(&conn, "atlas_lib", 50).unwrap().items.is_empty());
    assert_eq!(search::run(&conn, "flag_check", 50).unwrap().items.len(), 2);
    // Content decides the type: the Wavefront model is CAD, the MSVC objects aren't.
    let cad: Vec<String> = search::run(&conn, "type:cad", 50).unwrap().items.into_iter().map(|f| f.name).collect();
    assert_eq!(cad, vec!["cube.obj"]);
    let objs = search::run(&conn, "flag_check", 50).unwrap().items;
    assert!(objs.iter().all(|f| f.kind == "object"));

    // Treemap tiles carry their dominant category.
    let tm = dashboard::treemap(&conn, Some(root.to_string_lossy().to_string()), 60).unwrap();
    let cat = |name: &str| tm.children.iter().find(|c| c.name == name).map(|c| c.category).unwrap();
    assert_eq!(cat("Documents"), "documents");
    assert_eq!(cat("CAD"), "cad");
    assert_eq!(cat("Media"), "media");
    assert_eq!(cat("proj"), "code");
}
