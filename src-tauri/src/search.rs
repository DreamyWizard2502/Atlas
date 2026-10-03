//! Search over the SQLite index. Never touches the disk.
//!
//! Syntax: free text plus filters
//!   type:pdf  type:image  ext:sldprt,step  size:>500mb  size:<10kb
//!   modified:today | yesterday | this week | last week | this month | last month | this year | 7d
//!   in:"Space name"   folder:true (folders only)

use crate::model::{type_condition, FileItem, FILE_COLS};
use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection};
use serde::Serialize;

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedQuery {
    pub text: Vec<String>,
    /// `type:` values (matched by content-confirmed kind).
    pub types: Vec<String>,
    /// `ext:` values (matched by extension only).
    pub exts: Vec<String>,
    pub size_min: Option<i64>,
    pub size_max: Option<i64>,
    pub modified_from: Option<i64>,
    pub modified_to: Option<i64>,
    pub modified_label: Option<String>,
    pub space: Option<String>,
    pub folders_only: Option<bool>,
    pub chips: Vec<String>,
}

fn tokenize(q: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for c in q.chars() {
        match c {
            '"' => in_quote = !in_quote,
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn parse_size(s: &str) -> Option<i64> {
    let s = s.trim().to_lowercase();
    let idx = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(idx);
    let n: f64 = num.parse().ok()?;
    let mult: f64 = match unit.trim() {
        "" | "b" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" => 1024.0f64.powi(4),
        _ => return None,
    };
    Some((n * mult) as i64)
}

fn start_of_day(d: NaiveDate) -> i64 {
    Local
        .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

/// Resolve a relative date phrase to a `[from, to)` unix range in local time.
pub fn date_range(phrase: &str, today: NaiveDate) -> Option<(i64, Option<i64>)> {
    let p = phrase.trim().to_lowercase().replace(['-', '_'], " ");
    let p = p.split_whitespace().collect::<Vec<_>>().join(" ");
    let week_start = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let month_start = today.with_day(1)?;
    let prev_month_start = (month_start - Duration::days(1)).with_day(1)?;
    let year_start = NaiveDate::from_ymd_opt(today.year(), 1, 1)?;
    Some(match p.as_str() {
        "today" => (start_of_day(today), None),
        "yesterday" => (start_of_day(today - Duration::days(1)), Some(start_of_day(today))),
        "this week" | "thisweek" | "week" => (start_of_day(week_start), None),
        "last week" | "lastweek" => (start_of_day(week_start - Duration::days(7)), Some(start_of_day(week_start))),
        "this month" | "thismonth" | "month" => (start_of_day(month_start), None),
        "last month" | "lastmonth" => (start_of_day(prev_month_start), Some(start_of_day(month_start))),
        "this year" | "thisyear" | "year" => (start_of_day(year_start), None),
        "last year" | "lastyear" => {
            (start_of_day(NaiveDate::from_ymd_opt(today.year() - 1, 1, 1)?), Some(start_of_day(year_start)))
        }
        other => {
            let n: i64 = other.strip_suffix('d').or_else(|| other.strip_suffix(" days"))?.trim().parse().ok()?;
            (start_of_day(today - Duration::days(n)), None)
        }
    })
}

pub fn parse(q: &str) -> ParsedQuery {
    parse_at(q, Local::now().date_naive())
}

pub fn parse_at(q: &str, today: NaiveDate) -> ParsedQuery {
    let mut pq = ParsedQuery::default();
    let toks = tokenize(q);
    let mut i = 0;
    while i < toks.len() {
        let tok = &toks[i];
        i += 1;
        let Some((key, val)) = tok.split_once(':') else {
            pq.text.push(tok.clone());
            continue;
        };
        let key = key.to_lowercase();
        let mut val = val.to_string();
        match key.as_str() {
            "type" | "kind" => {
                for v in val.split(',').filter(|s| !s.is_empty()) {
                    pq.types.push(v.to_lowercase());
                }
                pq.chips.push(format!("type: {val}"));
            }
            "ext" => {
                for v in val.split(',').filter(|s| !s.is_empty()) {
                    pq.exts.push(v.trim_start_matches('.').to_lowercase());
                }
                pq.chips.push(format!("ext: {val}"));
            }
            "size" => {
                let (op, num) = if let Some(r) = val.strip_prefix(">=") {
                    (">", r)
                } else if let Some(r) = val.strip_prefix("<=") {
                    ("<", r)
                } else if let Some(r) = val.strip_prefix('>') {
                    (">", r)
                } else if let Some(r) = val.strip_prefix('<') {
                    ("<", r)
                } else {
                    (">", val.as_str())
                };
                if let Some(n) = parse_size(num) {
                    if op == ">" {
                        pq.size_min = Some(n);
                    } else {
                        pq.size_max = Some(n);
                    }
                    pq.chips.push(format!("size {op} {num}"));
                } else {
                    pq.text.push(tok.clone());
                }
            }
            "modified" | "date" | "changed" => {
                // Allow `modified:this week` without quotes.
                if matches!(val.to_lowercase().as_str(), "this" | "last" | "past") && i < toks.len() {
                    val = format!("{} {}", val, toks[i]);
                    i += 1;
                }
                if let Some((from, to)) = date_range(&val, today) {
                    pq.modified_from = Some(from);
                    pq.modified_to = to;
                    pq.modified_label = Some(val.clone());
                    pq.chips.push(format!("modified: {val}"));
                } else {
                    pq.text.push(tok.clone());
                }
            }
            "in" | "space" => {
                if !val.is_empty() {
                    pq.chips.push(format!("in: {val}"));
                    pq.space = Some(val);
                }
            }
            "folder" | "is" => {
                let v = val.to_lowercase();
                if v == "folder" || v == "true" || v == "dir" {
                    pq.folders_only = Some(true);
                    pq.chips.push("folders".into());
                } else if v == "file" || v == "false" {
                    pq.folders_only = Some(false);
                    pq.chips.push("files".into());
                }
            }
            _ => pq.text.push(tok.clone()),
        }
    }
    pq
}

/// Build an FTS5 MATCH expression: each word becomes a quoted prefix term.
pub fn fts_query(words: &[String]) -> Option<String> {
    let terms: Vec<String> = words
        .iter()
        .flat_map(|w| {
            w.split(|c: char| !c.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .map(|s| format!("\"{}\"*", s.replace('"', "")))
                .collect::<Vec<_>>()
        })
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub items: Vec<FileItem>,
    pub parsed: ParsedQuery,
    pub unknown_space: bool,
    pub took_ms: u64,
}

fn filters_sql(conn: &Connection, pq: &ParsedQuery, vals: &mut Vec<Value>, unknown_space: &mut bool) -> Result<Vec<String>> {
    let mut w = Vec::new();
    let mut any_type = Vec::new();
    for t in &pq.types {
        any_type.push(type_condition(t, "f", vals));
    }
    if !pq.exts.is_empty() {
        any_type.push(format!("f.ext IN ({})", vec!["?"; pq.exts.len()].join(",")));
        vals.extend(pq.exts.iter().cloned().map(Value::Text));
    }
    if !any_type.is_empty() {
        w.push(format!("({})", any_type.join(" OR ")));
    }
    if let Some(n) = pq.size_min {
        w.push("f.size > ? AND f.is_dir = 0".into());
        vals.push(Value::Integer(n));
    }
    if let Some(n) = pq.size_max {
        w.push("f.size < ? AND f.is_dir = 0".into());
        vals.push(Value::Integer(n));
    }
    if let Some(n) = pq.modified_from {
        w.push("f.modified >= ?".into());
        vals.push(Value::Integer(n));
    }
    if let Some(n) = pq.modified_to {
        w.push("f.modified < ?".into());
        vals.push(Value::Integer(n));
    }
    match pq.folders_only {
        Some(true) => w.push("f.is_dir = 1".into()),
        Some(false) => w.push("f.is_dir = 0".into()),
        None => {}
    }
    if let Some(space) = &pq.space {
        match crate::spaces::where_for_name(conn, space)? {
            Some((sql, v)) => {
                w.push(sql);
                vals.extend(v);
            }
            None => {
                *unknown_space = true;
                w.push("0".into());
            }
        }
    }
    Ok(w)
}

pub fn run(conn: &Connection, q: &str, limit: i64) -> Result<SearchResult> {
    let started = std::time::Instant::now();
    let pq = parse(q);
    let mut unknown_space = false;
    let mut items = Vec::new();
    let recency = "(strftime('%s','now') - COALESCE(f.modified, 0)) / 31536000.0";

    if let Some(fts) = fts_query(&pq.text) {
        let mut vals = vec![Value::Text(fts)];
        let mut w = vec!["files_fts MATCH ?".to_string()];
        w.extend(filters_sql(conn, &pq, &mut vals, &mut unknown_space)?);
        vals.push(Value::Integer(limit));
        let sql = format!(
            "SELECT {FILE_COLS} FROM files_fts JOIN files f ON f.id = files_fts.rowid
             WHERE {} ORDER BY bm25(files_fts) + {recency} LIMIT ?",
            w.join(" AND ")
        );
        let mut stmt = conn.prepare(&sql)?;
        items = stmt.query_map(params_from_iter(vals), FileItem::from_row)?.collect::<Result<_, _>>()?;

        // Fallback: substring match for mid-word queries ("voice" → "invoice").
        if items.is_empty() {
            let mut vals = Vec::new();
            let mut w = Vec::new();
            for t in &pq.text {
                w.push("f.name LIKE ? ESCAPE '\\'".to_string());
                vals.push(Value::Text(format!(
                    "%{}%",
                    t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
                )));
            }
            w.extend(filters_sql(conn, &pq, &mut vals, &mut unknown_space)?);
            vals.push(Value::Integer(limit));
            let sql = format!("SELECT {FILE_COLS} FROM files f WHERE {} ORDER BY f.modified DESC LIMIT ?", w.join(" AND "));
            let mut stmt = conn.prepare(&sql)?;
            items = stmt.query_map(params_from_iter(vals), FileItem::from_row)?.collect::<Result<_, _>>()?;
        }
    } else if !pq.chips.is_empty() {
        let mut vals = Vec::new();
        let w = filters_sql(conn, &pq, &mut vals, &mut unknown_space)?;
        vals.push(Value::Integer(limit));
        let where_sql = if w.is_empty() { "1".to_string() } else { w.join(" AND ") };
        let order = if pq.size_min.is_some() { "f.size DESC" } else { "f.modified DESC" };
        let sql = format!("SELECT {FILE_COLS} FROM files f WHERE {where_sql} ORDER BY {order} LIMIT ?");
        let mut stmt = conn.prepare(&sql)?;
        items = stmt.query_map(params_from_iter(vals), FileItem::from_row)?.collect::<Result<_, _>>()?;
    }
    Ok(SearchResult { items, parsed: pq, unknown_space, took_ms: started.elapsed().as_millis() as u64 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parses_filters_and_text() {
        let pq = parse_at("bracket type:pdf size:>500mb modified:this week in:\"Client work\"", d(2026, 10, 2));
        assert_eq!(pq.text, vec!["bracket"]);
        assert_eq!(pq.types, vec!["pdf"]);
        assert_eq!(pq.size_min, Some(500 * 1024 * 1024));
        assert_eq!(pq.space.as_deref(), Some("Client work"));
        // 2026-10-02 is a Friday; the week starts Monday 2026-09-28.
        assert_eq!(pq.modified_from, Some(start_of_day(d(2026, 9, 28))));
        assert_eq!(pq.modified_to, None);
    }

    #[test]
    fn parses_ext_lists_and_type_categories() {
        let pq = parse_at("ext:sldprt,.STEP", d(2026, 1, 1));
        assert_eq!(pq.exts, vec!["sldprt", "step"]);
        let pq = parse_at("type:image", d(2026, 1, 1));
        assert_eq!(pq.types, vec!["image"]);
        assert!(pq.exts.is_empty());
        assert!(pq.text.is_empty());
    }

    #[test]
    fn last_month_crosses_year() {
        let (from, to) = date_range("last month", d(2026, 1, 15)).unwrap();
        assert_eq!(from, start_of_day(d(2025, 12, 1)));
        assert_eq!(to, Some(start_of_day(d(2026, 1, 1))));
    }

    #[test]
    fn size_units() {
        assert_eq!(parse_size("10kb"), Some(10 * 1024));
        assert_eq!(parse_size("1.5gb"), Some((1.5 * 1024.0 * 1024.0 * 1024.0) as i64));
        assert_eq!(parse_size("abc"), None);
    }

    #[test]
    fn fts_escaping() {
        assert_eq!(fts_query(&["inv_2024".into()]).unwrap(), "\"inv\"* \"2024\"*");
        assert_eq!(fts_query(&["--".into()]), None);
    }
}
