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

/// Days of history the growth chart shows.
pub const HISTORY_DAYS: i64 = 90;

/// Estimate the indexed total for each of the last `days` days from file
/// creation dates: the total on day D is today's total minus the bytes of files
/// created after D. Files deleted since aren't known, so this is a floor, not a
/// record; real daily snapshots replace it as they accumulate. Skipped once
/// real snapshots cover the whole range.
pub fn estimate_history(conn: &Connection, days: i64) -> Result<()> {
    let today = chrono::Local::now().date_naive();
    let start = today - chrono::Duration::days(days - 1);
    let start_s = start.format("%Y-%m-%d").to_string();
    let covered: Option<String> = conn.query_row("SELECT MIN(day) FROM snapshots WHERE kind = 'total'", [], |r| r.get(0))?;
    if covered.as_deref().map(|d| d <= start_s.as_str()).unwrap_or(false) {
        conn.execute("DELETE FROM snapshots WHERE kind = 'total_est'", [])?;
        return Ok(());
    }
    let (total, count): (i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(size), 0), COALESCE(SUM(file_count), 0) FROM files WHERE path = root",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let start_ts = chrono::TimeZone::from_local_datetime(&chrono::Local, &start.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or(0);
    let mut created_on: std::collections::HashMap<String, (i64, i64)> = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT date(created, 'unixepoch', 'localtime'), SUM(size), COUNT(*) FROM files
             WHERE is_dir = 0 AND created >= ?1 GROUP BY 1",
        )?;
        let rows = stmt.query_map([start_ts], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
        for row in rows {
            let (day, size, n) = row?;
            created_on.insert(day, (size, n));
        }
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM snapshots WHERE kind = 'total_est'", [])?;
    {
        let mut ins = tx.prepare("INSERT INTO snapshots(day, kind, key, size, count) VALUES (?1, 'total_est', '', ?2, ?3)")?;
        // Walk back from today, removing what was created on each later day.
        let (mut size, mut n) = (total, count);
        let mut day = today;
        while day >= start {
            let key = day.format("%Y-%m-%d").to_string();
            ins.execute(rusqlite::params![key, size.max(0), n.max(0)])?;
            if let Some((s, c)) = created_on.get(&key) {
                size -= s;
                n -= c;
            }
            day -= chrono::Duration::days(1);
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
