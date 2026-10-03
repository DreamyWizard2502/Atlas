//! Daily snapshots of size per top-level folder and per file type. Taking a
//! snapshot more than once a day just refreshes today's rows.

use anyhow::Result;
use rusqlite::{params, Connection};

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn take(conn: &Connection) -> Result<()> {
    let day = today();
    let roots: Vec<String> = {
        let mut stmt = conn.prepare("SELECT DISTINCT root FROM files")?;
        let v = stmt.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
        v
    };
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM snapshots WHERE day = ?1", [&day])?;
    let mut total = (0i64, 0i64);
    {
        let mut ins = tx.prepare("INSERT OR REPLACE INTO snapshots(day, kind, key, size, count) VALUES (?1, ?2, ?3, ?4, ?5)")?;
        for root in &roots {
            let (s, c): (i64, i64) = tx
                .query_row("SELECT size, file_count FROM files WHERE path = ?1", [root], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap_or((0, 0));
            total.0 += s;
            total.1 += c;
            let mut stmt = tx.prepare("SELECT path, size, file_count FROM files WHERE parent = ?1 AND is_dir = 1")?;
            let rows = stmt.query_map([root], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
            for row in rows {
                let (path, size, count) = row?;
                ins.execute(params![day, "folder", path, size, count])?;
            }
        }
        ins.execute(params![day, "total", "", total.0, total.1])?;
        let mut stmt = tx.prepare(
            "SELECT ext, SUM(size), COUNT(*) FROM files WHERE is_dir = 0 GROUP BY ext ORDER BY 2 DESC LIMIT 60",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
        for row in rows {
            let (ext, size, count) = row?;
            ins.execute(params![day, "type", ext, size, count])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn has_today(conn: &Connection) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM snapshots WHERE day = ?1 AND kind = 'total'", [today()], |_| Ok(true))
        .unwrap_or(false))
}
