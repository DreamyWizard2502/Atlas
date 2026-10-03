//! Spaces: smart collections defined by rules, plus explicitly added files.
//! Files are never moved; a space is just a saved query.

use crate::db::{now, subtree_range};
use crate::model::{type_condition, FileItem, FILE_COLS};
use anyhow::{anyhow, Result};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpaceRules {
    /// "all" (AND) or "any" (OR)
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

fn default_mode() -> String {
    "all".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// nameContains | ext | kind | folder | tag | modifiedWithinDays | modifiedAfter | modifiedBefore | sizeAboveMb
    pub field: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: i64,
    pub name: String,
    pub icon: String,
    pub rules: SpaceRules,
    pub pinned: bool,
    pub count: i64,
    pub size: i64,
    pub last_activity: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceInput {
    pub id: Option<i64>,
    pub name: String,
    pub icon: Option<String>,
    pub rules: SpaceRules,
    pub pinned: Option<bool>,
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn local_date_ts(s: &str) -> Option<i64> {
    use chrono::TimeZone;
    let d = chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?;
    chrono::Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest().map(|t| t.timestamp())
}

/// Compile one rule into a SQL condition over `files f`.
fn rule_sql(rule: &Rule, out: &mut Vec<Value>) -> Option<String> {
    let v = rule.value.trim();
    if v.is_empty() {
        return None;
    }
    Some(match rule.field.as_str() {
        "nameContains" => {
            out.push(Value::Text(format!("%{}%", like_escape(v))));
            "f.name LIKE ? ESCAPE '\\'".into()
        }
        "kind" => {
            let conds: Vec<String> =
                v.split([',', ' ']).filter(|s| !s.is_empty()).map(|t| type_condition(t, "f", out)).collect();
            if conds.is_empty() {
                return None;
            }
            format!("({})", conds.join(" OR "))
        }
        "ext" => {
            let exts: Vec<String> =
                v.split([',', ' ']).filter(|s| !s.is_empty()).map(|s| s.trim_start_matches('.').to_lowercase()).collect();
            if exts.is_empty() {
                return None;
            }
            let ph = vec!["?"; exts.len()].join(",");
            out.extend(exts.into_iter().map(Value::Text));
            format!("f.ext IN ({ph})")
        }
        "folder" => {
            let (lo, hi) = subtree_range(v);
            out.push(Value::Text(lo));
            out.push(Value::Text(hi));
            "(f.path >= ? AND f.path < ?)".into()
        }
        "tag" => {
            out.push(Value::Text(v.to_string()));
            "f.path IN (SELECT ft.path FROM file_tags ft JOIN tags t ON t.id = ft.tag_id WHERE t.name = ? COLLATE NOCASE)".into()
        }
        "modifiedWithinDays" => {
            let days: i64 = v.parse().ok()?;
            out.push(Value::Integer(now() - days * 86_400));
            "f.modified >= ?".into()
        }
        "modifiedAfter" => {
            out.push(Value::Integer(local_date_ts(v)?));
            "f.modified >= ?".into()
        }
        "modifiedBefore" => {
            out.push(Value::Integer(local_date_ts(v)? + 86_400));
            "f.modified < ?".into()
        }
        "sizeAboveMb" => {
            let mb: f64 = v.parse().ok()?;
            out.push(Value::Integer((mb * 1024.0 * 1024.0) as i64));
            "f.size >= ?".into()
        }
        _ => return None,
    })
}

/// WHERE fragment selecting the members of a space.
pub fn space_where(space_id: i64, rules: &SpaceRules) -> (String, Vec<Value>) {
    let mut vals = Vec::new();
    let parts: Vec<String> = rules.rules.iter().filter_map(|r| rule_sql(r, &mut vals)).collect();
    let joiner = if rules.mode == "any" { " OR " } else { " AND " };
    let rule_part = if parts.is_empty() { "0".to_string() } else { parts.join(joiner) };
    vals.push(Value::Integer(space_id));
    (
        format!("((f.is_dir = 0 AND ({rule_part})) OR f.path IN (SELECT path FROM space_items WHERE space_id = ?))"),
        vals,
    )
}

fn load_rules(conn: &Connection, id: i64) -> Result<SpaceRules> {
    let json: String = conn.query_row("SELECT rules_json FROM spaces WHERE id = ?1", [id], |r| r.get(0))?;
    Ok(serde_json::from_str(&json).unwrap_or_default())
}

pub fn where_for_name(conn: &Connection, name: &str) -> Result<Option<(String, Vec<Value>)>> {
    let id: Option<i64> = conn
        .query_row("SELECT id FROM spaces WHERE name = ?1 COLLATE NOCASE", [name], |r| r.get(0))
        .optional()?;
    let id = match id {
        Some(id) => Some(id),
        None => conn
            .query_row(
                "SELECT id FROM spaces WHERE name LIKE ?1 ESCAPE '\\' ORDER BY length(name) LIMIT 1",
                [format!("{}%", like_escape(name))],
                |r| r.get(0),
            )
            .optional()?,
    };
    match id {
        Some(id) => Ok(Some(space_where(id, &load_rules(conn, id)?))),
        None => Ok(None),
    }
}

pub fn list(conn: &Connection) -> Result<Vec<Space>> {
    let mut stmt = conn.prepare("SELECT id, name, icon, rules_json, pinned FROM spaces ORDER BY pinned DESC, name")?;
    let rows: Vec<(i64, String, String, String, bool)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, name, icon, json, pinned) in rows {
        let rules: SpaceRules = serde_json::from_str(&json).unwrap_or_default();
        let (w, vals) = space_where(id, &rules);
        let (count, size, last): (i64, i64, Option<i64>) = conn.query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(f.size), 0), MAX(f.modified) FROM files f WHERE {w}"),
            params_from_iter(vals),
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        out.push(Space { id, name, icon, rules, pinned, count, size, last_activity: last });
    }
    Ok(out)
}

/// Names of the spaces that include `path`. One indexed lookup per space; no
/// counting (unlike `list`).
pub fn names_containing(conn: &Connection, path: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id, name, rules_json FROM spaces ORDER BY name")?;
    let rows: Vec<(i64, String, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, name, json) in rows {
        let rules: SpaceRules = serde_json::from_str(&json).unwrap_or_default();
        let (w, mut vals) = space_where(id, &rules);
        vals.push(Value::Text(path.to_string()));
        let hit = conn
            .query_row(&format!("SELECT 1 FROM files f WHERE f.path = ? AND {w}"), params_from_iter(rotate_last_first(vals)), |_| Ok(()))
            .optional()?
            .is_some();
        if hit {
            out.push(name);
        }
    }
    Ok(out)
}

/// Move the last value to the front (the path binds first in `names_containing`).
fn rotate_last_first(mut v: Vec<Value>) -> Vec<Value> {
    if let Some(last) = v.pop() {
        v.insert(0, last);
    }
    v
}

pub fn save(conn: &Connection, input: &SpaceInput) -> Result<i64> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(anyhow!("Space name can't be empty"));
    }
    let json = serde_json::to_string(&input.rules)?;
    let icon = input.icon.clone().unwrap_or_else(|| "folder".into());
    match input.id {
        Some(id) => {
            conn.execute(
                "UPDATE spaces SET name = ?1, icon = ?2, rules_json = ?3, pinned = COALESCE(?4, pinned) WHERE id = ?5",
                params![name, icon, json, input.pinned, id],
            )?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO spaces(name, icon, rules_json, pinned, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![name, icon, json, input.pinned.unwrap_or(false), now()],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM space_items WHERE space_id = ?1", [id])?;
    conn.execute("DELETE FROM spaces WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> Result<()> {
    conn.execute("UPDATE spaces SET pinned = ?1 WHERE id = ?2", params![pinned, id])?;
    Ok(())
}

pub fn add_item(conn: &Connection, id: i64, path: &str) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO space_items(space_id, path) VALUES (?1, ?2)", params![id, path])?;
    Ok(())
}

pub fn remove_item(conn: &Connection, id: i64, path: &str) -> Result<()> {
    conn.execute("DELETE FROM space_items WHERE space_id = ?1 AND path = ?2", params![id, path])?;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceFiles {
    pub items: Vec<FileItem>,
    pub total: i64,
}

pub fn files(conn: &Connection, id: i64, sort: &str, limit: i64, offset: i64) -> Result<SpaceFiles> {
    let rules = load_rules(conn, id)?;
    let (w, vals) = space_where(id, &rules);
    let order = match sort {
        "name" => "f.name COLLATE NOCASE ASC",
        "size" => "f.size DESC",
        _ => "MAX(COALESCE(f.modified, 0), COALESCE((SELECT opened_at FROM activity a WHERE a.path = f.path), 0)) DESC",
    };
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM files f WHERE {w}"),
        params_from_iter(vals.clone()),
        |r| r.get(0),
    )?;
    let mut all = vals;
    all.push(Value::Integer(limit));
    all.push(Value::Integer(offset));
    let mut stmt = conn.prepare(&format!("SELECT {FILE_COLS} FROM files f WHERE {w} ORDER BY {order} LIMIT ? OFFSET ?"))?;
    let items = stmt.query_map(params_from_iter(all), FileItem::from_row)?.collect::<Result<_, _>>()?;
    Ok(SpaceFiles { items, total })
}

// ---- tags ----

pub fn list_tags(conn: &Connection) -> Result<Vec<(i64, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, (SELECT COUNT(*) FROM file_tags ft WHERE ft.tag_id = t.id) FROM tags t ORDER BY t.name",
    )?;
    let v = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

pub fn add_tag(conn: &Connection, path: &str, tag: &str) -> Result<()> {
    let tag = tag.trim();
    if tag.is_empty() {
        return Err(anyhow!("Tag can't be empty"));
    }
    conn.execute("INSERT OR IGNORE INTO tags(name) VALUES (?1)", [tag])?;
    let id: i64 = conn.query_row("SELECT id FROM tags WHERE name = ?1", [tag], |r| r.get(0))?;
    conn.execute("INSERT OR IGNORE INTO file_tags(path, tag_id) VALUES (?1, ?2)", params![path, id])?;
    Ok(())
}

pub fn remove_tag(conn: &Connection, path: &str, tag: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM file_tags WHERE path = ?1 AND tag_id = (SELECT id FROM tags WHERE name = ?2)",
        params![path, tag],
    )?;
    Ok(())
}

pub fn tags_for(conn: &Connection, path: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT t.name FROM tags t JOIN file_tags ft ON ft.tag_id = t.id WHERE ft.path = ?1 ORDER BY t.name")?;
    let v = stmt.query_map([path], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn rules_select_expected_files() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(tmp.path().join("t.db")).unwrap();
        let conn = db.conn().unwrap();
        let base = tmp.path().join("proj");
        let base_s = base.to_string_lossy().to_string();
        for (name, ext, size) in [("Bracket v2.sldprt", "sldprt", 10), ("notes.txt", "txt", 5), ("bracket.pdf", "pdf", 7)] {
            let p = base.join(name).to_string_lossy().to_string();
            conn.execute(
                "INSERT INTO files(path, parent, name, ext, is_dir, size, modified, root) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?2)",
                params![p, base_s, name, ext, size, now()],
            )
            .unwrap();
        }
        let id = save(
            &conn,
            &SpaceInput {
                id: None,
                name: "Bracket".into(),
                icon: None,
                pinned: Some(true),
                rules: SpaceRules {
                    mode: "all".into(),
                    rules: vec![
                        Rule { field: "nameContains".into(), value: "bracket".into() },
                        Rule { field: "kind".into(), value: "cad".into() },
                    ],
                },
            },
        )
        .unwrap();
        let res = files(&conn, id, "recent", 50, 0).unwrap();
        assert_eq!(res.total, 1);
        assert_eq!(res.items[0].name, "Bracket v2.sldprt");
        add_item(&conn, id, &base.join("notes.txt").to_string_lossy()).unwrap();
        assert_eq!(files(&conn, id, "name", 50, 0).unwrap().total, 2);
        add_tag(&conn, &base.join("bracket.pdf").to_string_lossy(), "client").unwrap();
        let tagged = SpaceRules { mode: "any".into(), rules: vec![Rule { field: "tag".into(), value: "Client".into() }] };
        let (w, vals) = space_where(999, &tagged);
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM files f WHERE {w}"), params_from_iter(vals), |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
