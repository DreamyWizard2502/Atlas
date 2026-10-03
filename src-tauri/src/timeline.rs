//! Change feed: file modifications from the index plus deletions observed by the
//! watcher, newest first, paged by timestamp.

use crate::model::{exts_for_type, kind_of};
use anyhow::Result;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TimelineFilter {
    pub kind: Option<String>,
    pub space_id: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub ts: i64,
    pub change: String,
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: i64,
    pub kind: &'static str,
    pub exists: bool,
}

pub fn page(conn: &Connection, before: Option<i64>, limit: i64, filter: &TimelineFilter) -> Result<Vec<TimelineEntry>> {
    let before = before.unwrap_or(i64::MAX);
    let exts: Vec<String> = filter.kind.as_deref().filter(|k| !k.is_empty()).map(exts_for_type).unwrap_or_default();
    let mut file_where = vec!["f.is_dir = 0".to_string(), "f.modified < ?".to_string()];
    let mut vals: Vec<Value> = vec![Value::Integer(before)];
    if !exts.is_empty() {
        file_where.push(format!("f.ext IN ({})", vec!["?"; exts.len()].join(",")));
        vals.extend(exts.iter().cloned().map(Value::Text));
    }
    let mut include_deleted = true;
    if let Some(id) = filter.space_id {
        let json: String = conn.query_row("SELECT rules_json FROM spaces WHERE id = ?1", [id], |r| r.get(0))?;
        let rules = serde_json::from_str(&json).unwrap_or_default();
        let (w, v) = crate::spaces::space_where(id, &rules);
        file_where.push(w);
        vals.extend(v);
        include_deleted = false;
    }
    let mut sql = format!(
        "SELECT f.modified AS ts,
                CASE WHEN ABS(COALESCE(f.created, 0) - f.modified) <= 5 THEN 'created' ELSE 'modified' END,
                f.path, f.name, f.ext, f.size, 1
         FROM files f WHERE {}",
        file_where.join(" AND ")
    );
    if include_deleted {
        sql.push_str(" UNION ALL SELECT e.ts, e.kind, e.path, e.name, e.ext, e.size, 0 FROM events e WHERE e.kind = 'deleted' AND e.ts < ?");
        vals.push(Value::Integer(before));
        if !exts.is_empty() {
            sql.push_str(&format!(" AND e.ext IN ({})", vec!["?"; exts.len()].join(",")));
            vals.extend(exts.iter().cloned().map(Value::Text));
        }
    }
    sql.push_str(" ORDER BY ts DESC LIMIT ?");
    vals.push(Value::Integer(limit));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(vals), |r| {
        let ext: String = r.get(4)?;
        Ok(TimelineEntry {
            ts: r.get(0)?,
            change: r.get(1)?,
            path: r.get(2)?,
            name: r.get(3)?,
            kind: kind_of(&ext),
            ext,
            size: r.get(5)?,
            exists: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
