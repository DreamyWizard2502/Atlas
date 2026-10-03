use rusqlite::types::Value;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileItem {
    pub id: i64,
    pub path: String,
    pub parent: String,
    pub name: String,
    pub ext: String,
    pub is_dir: bool,
    pub size: i64,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub accessed: Option<i64>,
    pub kind: &'static str,
}

/// Columns expected by `FileItem::from_row`, for a `files` table aliased as `f`.
pub const FILE_COLS: &str = "f.id, f.path, f.parent, f.name, f.ext, f.is_dir, f.size, f.created, f.modified, f.accessed, f.kind";
/// Index of the first column after `FILE_COLS`.
pub const FILE_COLS_LEN: usize = 11;

impl FileItem {
    pub fn from_row(r: &rusqlite::Row) -> rusqlite::Result<FileItem> {
        let ext: String = r.get(4)?;
        let is_dir: bool = r.get(5)?;
        let stored: Option<String> = r.get(10)?;
        Ok(FileItem {
            id: r.get(0)?,
            path: r.get(1)?,
            parent: r.get(2)?,
            name: r.get(3)?,
            kind: if is_dir { "folder" } else { stored_kind(stored.as_deref(), &ext) },
            ext,
            is_dir,
            size: r.get(6)?,
            created: r.get(7)?,
            modified: r.get(8)?,
            accessed: r.get(9)?,
        })
    }
}

/// File type categories, used by `type:` search filters and type icons.
pub const KINDS: &[(&str, &[&str])] = &[
    ("image", &["jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "svg", "ico", "raw", "cr2", "nef", "psd"]),
    ("video", &["mp4", "mov", "mkv", "avi", "wmv", "webm", "m4v", "flv"]),
    ("audio", &["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma", "opus"]),
    ("pdf", &["pdf"]),
    ("doc", &["doc", "docx", "odt", "rtf", "pages"]),
    ("sheet", &["xls", "xlsx", "xlsm", "ods", "csv", "numbers"]),
    ("slides", &["ppt", "pptx", "odp", "key"]),
    ("text", &["txt", "md", "log", "ini", "cfg", "conf"]),
    ("code", &[
        "rs", "ts", "tsx", "js", "jsx", "py", "java", "c", "cpp", "h", "hpp", "cs", "go", "rb", "php", "swift", "kt",
        "json", "yaml", "yml", "toml", "xml", "html", "css", "scss", "sql", "sh", "ps1", "bat", "lua", "vue",
    ]),
    ("archive", &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "cab"]),
    ("cad", &[
        "sldprt", "sldasm", "slddrw", "dwg", "dxf", "step", "stp", "iges", "igs", "stl", "3mf", "obj", "fbx", "ipt",
        "iam", "idw", "f3d", "3dm", "skp", "blend", "catpart", "prt", "x_t",
    ]),
    ("installer", &["exe", "msi", "msix", "appx", "appinstaller"]),
    ("font", &["ttf", "otf", "woff", "woff2"]),
];

/// Kinds that only content sniffing can assign.
pub const SNIFFED_KINDS: &[&str] = &["object"];

/// The kind stored by the indexer (content-confirmed), or the extension's kind
/// for rows indexed before kinds were stored.
pub fn stored_kind(stored: Option<&str>, ext: &str) -> &'static str {
    match stored {
        Some(k) => KINDS
            .iter()
            .map(|(name, _)| *name)
            .chain(SNIFFED_KINDS.iter().copied())
            .chain(["folder", "other"])
            .find(|name| *name == k)
            .unwrap_or("other"),
        None => kind_of(ext),
    }
}

/// SQL condition (over alias `a`) for files of type `t` (a `type:` value or a
/// space "kind" rule). Known types use the stored, content-confirmed kind, so a
/// compiled `.obj` is never counted as CAD; unknown names match an extension.
pub fn type_condition(t: &str, a: &str, vals: &mut Vec<Value>) -> String {
    let exts = exts_for_type(t);
    match canonical_type(t) {
        Some(kind) => {
            vals.push(Value::Text(kind.to_string()));
            vals.extend(exts.iter().cloned().map(Value::Text));
            format!("({a}.kind = ? OR ({a}.kind IS NULL AND {a}.ext IN ({})))", vec!["?"; exts.len()].join(","))
        }
        None => {
            vals.extend(exts.into_iter().map(Value::Text));
            format!("{a}.ext = ?")
        }
    }
}

pub fn kind_of(ext: &str) -> &'static str {
    for (kind, exts) in KINDS {
        if exts.contains(&ext) {
            return kind;
        }
    }
    "other"
}

/// The kind name for a `type:` value, if it names a known kind.
pub fn canonical_type(t: &str) -> Option<&'static str> {
    let alias = type_alias(&t.to_lowercase());
    KINDS.iter().map(|(k, _)| *k).find(|k| *k == alias)
}

fn type_alias(t: &str) -> String {
    match t {
        "images" | "photo" | "photos" | "picture" | "pictures" => "image",
        "videos" | "movie" | "movies" => "video",
        "music" | "sound" => "audio",
        "document" | "documents" | "docs" | "word" => "doc",
        "spreadsheet" | "spreadsheets" | "excel" | "sheets" => "sheet",
        "presentation" | "powerpoint" => "slides",
        "archives" | "compressed" => "archive",
        "installers" | "exe" | "setup" => "installer",
        "3d" | "model" | "models" => "cad",
        other => other,
    }
    .to_string()
}

/// Extensions for a `type:` value. Unknown names are treated as an extension.
pub fn exts_for_type(t: &str) -> Vec<String> {
    let alias = type_alias(&t.to_lowercase());
    for (kind, exts) in KINDS {
        if *kind == alias {
            return exts.iter().map(|s| s.to_string()).collect();
        }
    }
    vec![alias.trim_start_matches('.').to_string()]
}
