//! User-editable rules for app-generated junk (CAD backups, autosaves, lock
//! files, Office `~$` files...). Stored in settings so patterns can be added later.
//!
//! Pattern syntax: `*` (any run of characters) and `?` (one character), matched
//! case-insensitively against the file name. A pattern containing `\` or `/`
//! is matched against the full path instead (with an implicit leading `*`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JunkRule {
    pub name: String,
    pub pattern: String,
    pub enabled: bool,
    /// Only match files not modified for at least this many days.
    #[serde(default)]
    pub min_age_days: u32,
}

fn rule(name: &str, pattern: &str, min_age_days: u32) -> JunkRule {
    JunkRule { name: name.into(), pattern: pattern.into(), enabled: true, min_age_days }
}

pub fn default_junk_rules() -> Vec<JunkRule> {
    vec![
        rule("Office temp / owner files", "~$*", 1),
        rule("LibreOffice lock files", ".~lock.*#", 1),
        rule("SolidWorks backups", "Backup of *.sld*", 7),
        rule("SolidWorks autorecover", "*.swar", 7),
        rule("AutoCAD backups", "*.bak", 7),
        rule("AutoCAD autosaves", "*.sv$", 1),
        rule("AutoCAD lock files", "*.dwl", 1),
        rule("AutoCAD lock files", "*.dwl2", 1),
        rule("AutoCAD temp", "*.ac$", 1),
        rule("Rhino backups", "*.3dmbak", 7),
        rule("Rhino lock files", "*.rhl", 1),
        rule("Inventor old versions", r"\OldVersions\*", 30),
        rule("KiCad backups", "*-bak", 7),
        rule("KiCad autosaves", "_autosave-*", 7),
        rule("FreeCAD backups", "*.FCBak", 7),
        rule("SketchUp backups", "*.skb", 7),
        rule("Generic lock files", "*.lck", 1),
        rule("Unfinished downloads", "*.crdownload", 7),
        rule("Unfinished downloads", "*.part", 7),
        rule("macOS metadata", ".DS_Store", 0),
        JunkRule { name: "Temp files".into(), pattern: "*.tmp".into(), enabled: false, min_age_days: 7 },
    ]
}

/// Case-insensitive glob match with `*` and `?`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    glob_match_chars(&p, &t)
}

/// Glob match on pre-lowercased character slices (hot path for analysis).
pub fn glob_match_chars(p: &[char], t: &[char]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

impl JunkRule {
    pub fn matches(&self, path: &str, name: &str) -> bool {
        if !self.enabled || self.pattern.trim().is_empty() {
            return false;
        }
        let pat = self.pattern.trim();
        if pat.contains('\\') || pat.contains('/') {
            let norm_pat = pat.replace('/', "\\");
            let norm_path = path.replace('/', "\\");
            let anchored = if norm_pat.starts_with('*') { norm_pat } else { format!("*{norm_pat}") };
            glob_match(&anchored, &norm_path)
        } else {
            glob_match(pat, name)
        }
    }

    pub fn old_enough(&self, modified: Option<i64>, now: i64) -> bool {
        match modified {
            Some(m) => now - m >= self.min_age_days as i64 * 86_400,
            None => self.min_age_days == 0,
        }
    }
}

/// Rules pre-compiled for matching against many files.
pub struct CompiledRules<'a> {
    rules: Vec<(&'a JunkRule, Vec<char>, bool)>,
}

impl<'a> CompiledRules<'a> {
    pub fn new(rules: &'a [JunkRule]) -> Self {
        let rules = rules
            .iter()
            .filter(|r| r.enabled && !r.pattern.trim().is_empty())
            .map(|r| {
                let pat = r.pattern.trim().to_lowercase();
                let is_path = pat.contains('\\') || pat.contains('/');
                let pat = if is_path {
                    let p = pat.replace('/', "\\");
                    if p.starts_with('*') { p } else { format!("*{p}") }
                } else {
                    pat
                };
                (r, pat.chars().collect(), is_path)
            })
            .collect();
        CompiledRules { rules }
    }

    pub fn find(&self, path: &str, name: &str, modified: Option<i64>, now: i64) -> Option<&'a JunkRule> {
        let name_l: Vec<char> = name.to_lowercase().chars().collect();
        let mut path_l: Option<Vec<char>> = None;
        for (rule, pat, is_path) in &self.rules {
            let hit = if *is_path {
                let pl = path_l.get_or_insert_with(|| path.to_lowercase().replace('/', "\\").chars().collect());
                glob_match_chars(pat, pl)
            } else {
                glob_match_chars(pat, &name_l)
            };
            if hit && rule.old_enough(modified, now) {
                return Some(rule);
            }
        }
        None
    }
}

/// First enabled rule that matches a file, if any.
pub fn match_rules<'a>(rules: &'a [JunkRule], path: &str, name: &str, modified: Option<i64>, now: i64) -> Option<&'a JunkRule> {
    rules.iter().find(|r| r.matches(path, name) && r.old_enough(modified, now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basics() {
        assert!(glob_match("*.bak", "drawing.BAK"));
        assert!(glob_match("~$*", "~$report.docx"));
        assert!(!glob_match("~$*", "report~$.docx"));
        assert!(glob_match("*.sv$", "plan_1_1_1234.sv$"));
        assert!(glob_match("file?.txt", "file1.txt"));
        assert!(!glob_match("file?.txt", "file10.txt"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("*.dwl", "plan.dwl2"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
    }

    #[test]
    fn compiled_rules_agree_with_simple_matcher() {
        let rules = default_junk_rules();
        let compiled = CompiledRules::new(&rules);
        let now = 1_000_000_000;
        let old = Some(now - 40 * 86_400);
        for path in [r"C:\w\~$a.docx", r"C:\cad\x.DWL", r"C:\inv\OldVersions\p.ipt", r"C:\a\keep.txt", r"C:\a\b.3dmbak"] {
            let name = path.rsplit('\\').next().unwrap();
            assert_eq!(
                compiled.find(path, name, old, now).map(|r| r.name.clone()),
                match_rules(&rules, path, name, old, now).map(|r| r.name.clone()),
                "{path}"
            );
        }
    }

    #[test]
    fn default_rules_match_expected_files() {
        let rules = default_junk_rules();
        let now = 1_000_000_000;
        let old = Some(now - 40 * 86_400);
        let fresh = Some(now - 3600);
        let m = |path: &str, modified| {
            let name = path.rsplit(['\\', '/']).next().unwrap();
            match_rules(&rules, path, name, modified, now).map(|r| r.name.clone())
        };
        assert_eq!(m(r"C:\work\~$Quote.docx", old).as_deref(), Some("Office temp / owner files"));
        // Fresh lock files belong to documents that are probably still open.
        assert_eq!(m(r"C:\work\~$Quote.docx", fresh), None);
        assert_eq!(m(r"C:\cad\Backup of Bracket.SLDPRT", old).as_deref(), Some("SolidWorks backups"));
        assert_eq!(m(r"C:\cad\plan.dwl2", old).as_deref(), Some("AutoCAD lock files"));
        assert_eq!(m(r"C:\cad\part.3dmbak", old).as_deref(), Some("Rhino backups"));
        assert_eq!(m(r"C:\inv\OldVersions\part.0003.ipt", old).as_deref(), Some("Inventor old versions"));
        // Not junk.
        assert_eq!(m(r"C:\cad\Bracket.SLDPRT", old), None);
        assert_eq!(m(r"C:\notes\backup plan.txt", old), None);
        assert_eq!(m(r"C:\inv\OldVersionsNotes.txt", old), None);
        // Disabled by default.
        assert_eq!(m(r"C:\x\a.tmp", old), None);
    }

    #[test]
    fn disabled_and_empty_rules_never_match() {
        let r = JunkRule { name: "x".into(), pattern: "".into(), enabled: true, min_age_days: 0 };
        assert!(!r.matches("C:\\a", "a"));
        let r = JunkRule { name: "x".into(), pattern: "*".into(), enabled: false, min_age_days: 0 };
        assert!(!r.matches("C:\\a", "a"));
    }
}
