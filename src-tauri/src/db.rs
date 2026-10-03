//! SQLite storage. Every caller opens its own connection (cheap, WAL mode) so the
//! indexer's write batches never block UI reads.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Db {
    path: PathBuf,
}

const SCHEMA_VERSION: i64 = 2;

/// v2: content-sniffed kind and storage category per file; per-category byte
/// totals per folder (treemap colors). Filled in by the next scan.
const MIGRATE_V2: &str = r#"
ALTER TABLE files ADD COLUMN kind TEXT;
ALTER TABLE files ADD COLUMN cat INTEGER NOT NULL DEFAULT 7;
ALTER TABLE files ADD COLUMN cb0 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE files ADD COLUMN cb1 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE files ADD COLUMN cb2 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE files ADD COLUMN cb3 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE files ADD COLUMN cb4 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE files ADD COLUMN cb5 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE files ADD COLUMN cb6 INTEGER NOT NULL DEFAULT 0;
ALTER TABLE files ADD COLUMN cb7 INTEGER NOT NULL DEFAULT 0;
"#;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    parent TEXT NOT NULL,
    name TEXT NOT NULL,
    ext TEXT NOT NULL DEFAULT '',
    is_dir INTEGER NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    file_count INTEGER NOT NULL DEFAULT 0,
    created INTEGER,
    modified INTEGER,
    accessed INTEGER,
    attrs INTEGER NOT NULL DEFAULT 0,
    root TEXT NOT NULL,
    gen INTEGER NOT NULL DEFAULT 0,
    hash TEXT,
    hash_key TEXT
);
CREATE INDEX IF NOT EXISTS files_parent ON files(parent);
CREATE INDEX IF NOT EXISTS files_ext ON files(ext);
CREATE INDEX IF NOT EXISTS files_modified ON files(modified);
CREATE INDEX IF NOT EXISTS files_size ON files(size);
CREATE INDEX IF NOT EXISTS files_root_gen ON files(root, gen);

CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
    name, content='files', content_rowid='id',
    tokenize = 'unicode61 remove_diacritics 2', prefix = '2 3'
);
CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
END;
CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
    INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
END;
CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE OF name ON files BEGIN
    INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
    INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
END;

CREATE TABLE IF NOT EXISTS snapshots (
    day TEXT NOT NULL,
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    size INTEGER NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (day, kind, key)
);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    ts INTEGER NOT NULL,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    ext TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS events_ts ON events(ts);

CREATE TABLE IF NOT EXISTS activity (
    path TEXT PRIMARY KEY,
    opened_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);
CREATE TABLE IF NOT EXISTS file_tags (
    path TEXT NOT NULL,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (path, tag_id)
);
CREATE INDEX IF NOT EXISTS file_tags_tag ON file_tags(tag_id);

CREATE TABLE IF NOT EXISTS spaces (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE,
    icon TEXT NOT NULL DEFAULT 'folder',
    rules_json TEXT NOT NULL,
    pinned INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS space_items (
    space_id INTEGER NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    PRIMARY KEY (space_id, path)
);

CREATE TABLE IF NOT EXISTS holding (
    id INTEGER PRIMARY KEY,
    batch_id TEXT NOT NULL,
    original_path TEXT NOT NULL,
    held_path TEXT NOT NULL,
    size INTEGER NOT NULL,
    category TEXT NOT NULL,
    is_dir INTEGER NOT NULL DEFAULT 0,
    held_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    status TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS holding_status ON holding(status, expires_at);

CREATE TABLE IF NOT EXISTS cleanup_log (
    id INTEGER PRIMARY KEY,
    ts INTEGER NOT NULL,
    batch_id TEXT NOT NULL,
    action TEXT NOT NULL,
    path TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    dry_run INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS cleanup_log_ts ON cleanup_log(ts);

CREATE TABLE IF NOT EXISTS inbox_dismissed (
    path TEXT PRIMARY KEY,
    at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS kv (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Db { path: path.as_ref().to_path_buf() };
        if let Some(parent) = db.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = db.conn()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(SCHEMA)?;
        }
        if version < 2 {
            conn.execute_batch(&format!("BEGIN; {MIGRATE_V2} COMMIT;"))?;
        }
        if version < SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(db)
    }

    pub fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path)?;
        conn.busy_timeout(std::time::Duration::from_secs(15))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "cache_size", -32000)?;
        Ok(conn)
    }
}

pub fn kv_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0))
        .optional()?)
}

pub fn kv_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO kv(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// `[lo, hi)` string range covering every path strictly inside `dir`.
/// Works because the separator is followed by the next code point in ASCII
/// ('\\' → ']', '/' → '0').
pub fn subtree_range(dir: &str) -> (String, String) {
    let sep = std::path::MAIN_SEPARATOR;
    let base = dir.trim_end_matches(sep);
    let next = char::from_u32(sep as u32 + 1).unwrap();
    (format!("{base}{sep}"), format!("{base}{next}"))
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn to_unix(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    let t = t.ok()?;
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Some(d.as_secs() as i64),
        Err(e) => Some(-(e.duration().as_secs() as i64)),
    }
}

/// Lowercase extension without the dot.
pub fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtree_range_excludes_siblings() {
        let (lo, hi) = subtree_range(r"C:\a\b");
        if cfg!(windows) {
            assert!(r"C:\a\b\c.txt" >= lo.as_str() && r"C:\a\b\c.txt" < hi.as_str());
            assert!(!(r"C:\a\bc\x" >= lo.as_str() && r"C:\a\bc\x" < hi.as_str()));
        }
    }

    #[test]
    fn migrates_a_v1_database() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("v1.db");
        {
            // A database as the first release created it.
            let c = Connection::open(&path).unwrap();
            c.execute_batch(SCHEMA).unwrap();
            c.pragma_update(None, "user_version", 1).unwrap();
            c.execute(
                r"INSERT INTO files(path, parent, name, ext, is_dir, size, root) VALUES ('C:\a.txt', 'C:\', 'a.txt', 'txt', 0, 5, 'C:\')",
                [],
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let c = db.conn().unwrap();
        let v: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let (kind, cat, cb7): (Option<String>, i64, i64) =
            c.query_row("SELECT kind, cat, cb7 FROM files", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!((kind, cat, cb7), (None, 7, 0));
        drop(c);
        // Opening again is a no-op.
        Db::open(&path).unwrap();
    }

    #[test]
    fn ext_parsing() {
        assert_eq!(ext_of("a.PDF"), "pdf");
        assert_eq!(ext_of(".gitignore"), "");
        assert_eq!(ext_of("noext"), "");
        assert_eq!(ext_of("x.tar.gz"), "gz");
    }
}
