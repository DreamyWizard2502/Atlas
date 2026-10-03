//! Quick Look support: file details and bounded text reads. Media is streamed
//! to the webview through Tauri's asset protocol.

use crate::db::{ext_of, to_unix};
use crate::platform;
use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDetails {
    pub path: String,
    pub name: String,
    pub parent: String,
    pub ext: String,
    pub kind: &'static str,
    pub is_dir: bool,
    pub size: i64,
    pub file_count: Option<i64>,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub accessed: Option<i64>,
    pub readonly: bool,
    pub hidden: bool,
    pub tags: Vec<String>,
    pub spaces: Vec<String>,
}

pub fn details(conn: &Connection, path: &str) -> Result<FileDetails> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| anyhow!(friendly_io(&e)))?;
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.to_string());
    let ext = if meta.is_dir() { String::new() } else { ext_of(&name) };
    let indexed: Option<(i64, i64)> = conn
        .query_row("SELECT size, file_count FROM files WHERE path = ?1", [path], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok();
    let spaces = crate::spaces::names_containing(conn, path).unwrap_or_default();
    Ok(FileDetails {
        path: path.to_string(),
        parent: p.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
        kind: if meta.is_dir() { "folder" } else { crate::classify::kind_for(p, &ext) },
        is_dir: meta.is_dir(),
        size: if meta.is_dir() { indexed.map(|i| i.0).unwrap_or(0) } else { meta.len() as i64 },
        file_count: if meta.is_dir() { indexed.map(|i| i.1) } else { None },
        created: to_unix(meta.created()),
        modified: to_unix(meta.modified()),
        accessed: to_unix(meta.accessed()),
        readonly: meta.permissions().readonly(),
        hidden: platform::attrs(&meta) & platform::ATTR_HIDDEN != 0,
        tags: crate::spaces::tags_for(conn, path).unwrap_or_default(),
        spaces,
        ext,
        name,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    pub text: String,
    pub truncated: bool,
}

pub fn read_text(path: &str, max_bytes: usize) -> Result<TextPreview> {
    let meta = std::fs::metadata(path).map_err(|e| anyhow!(friendly_io(&e)))?;
    if platform::attrs(&meta) & platform::ATTR_CLOUD != 0 {
        return Err(anyhow!("This file is stored online only; open it to download"));
    }
    let mut f = std::fs::File::open(path).map_err(|e| anyhow!(friendly_io(&e)))?;
    let mut buf = Vec::with_capacity(max_bytes.min(meta.len() as usize));
    // The file can shrink, vanish or get locked mid-read; that's an error, not a crash.
    f.by_ref().take(max_bytes as u64).read_to_end(&mut buf).map_err(|e| anyhow!(friendly_io(&e)))?;
    if buf.iter().take(8000).any(|&b| b == 0) {
        // UTF-16 text (with BOM) is common on Windows.
        if buf.starts_with(&[0xFF, 0xFE]) {
            let units: Vec<u16> = buf[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            return Ok(TextPreview { text: String::from_utf16_lossy(&units), truncated: meta.len() as usize > max_bytes });
        }
        return Err(anyhow!("Binary file"));
    }
    Ok(TextPreview {
        text: String::from_utf8_lossy(&buf).to_string(),
        truncated: meta.len() as usize > max_bytes,
    })
}

// ---------------------------------------------------------------------------
// Quick Look probe: decide how to preview a file from its contents.

/// Largest image Quick Look/thumbnails will load (the asset protocol reads
/// whole files into memory).
pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub exists: bool,
    pub is_dir: bool,
    pub size: i64,
    /// "image" | "video" | "audio" | "pdf" | "text" | "none"
    pub mode: &'static str,
    pub kind: &'static str,
    /// Why there's no preview, in plain language.
    pub note: Option<String>,
}

/// User-facing wording for I/O errors (never raw OS codes).
pub fn friendly_io(e: &std::io::Error) -> String {
    match (e.kind(), e.raw_os_error()) {
        (std::io::ErrorKind::NotFound, _) => "This file was moved or deleted.".into(),
        (std::io::ErrorKind::PermissionDenied, _) => "Atlas doesn't have permission to read this file.".into(),
        (_, Some(32)) | (_, Some(33)) => "Another app is using this file right now.".into(),
        _ => "Atlas couldn't read this file.".into(),
    }
}

fn expected_mode(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "ico" | "avif" | "svg" => "image",
        "mp4" | "m4v" | "mov" | "webm" | "ogv" => "video",
        "mp3" | "wav" | "ogg" | "oga" | "m4a" | "flac" | "aac" | "opus" => "audio",
        "pdf" => "pdf",
        _ => match crate::model::kind_of(ext) {
            "text" | "code" => "text",
            _ => "none",
        },
    }
}

/// What the first bytes say the file is, if it's a previewable media type.
pub fn sniff_media(head: &[u8], ext: &str) -> Option<&'static str> {
    let at = |i: usize, s: &[u8]| head.len() >= i + s.len() && &head[i..i + s.len()] == s;
    if at(0, b"\x89PNG\r\n\x1a\n") || at(0, &[0xff, 0xd8, 0xff]) || at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some("image");
    }
    if at(0, b"BM") && head.len() >= 26 || at(0, &[0, 0, 1, 0]) && ext == "ico" {
        return Some("image");
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some("image");
    }
    if at(0, b"RIFF") && at(8, b"WAVE") {
        return Some("audio");
    }
    if at(4, b"ftyp") {
        let brand = head.get(8..12).unwrap_or(&[]);
        return match brand {
            b"avif" | b"avis" => Some("image"),
            b"heic" | b"heix" | b"mif1" | b"msf1" => None, // WebView2 can't render HEIF
            b"M4A " | b"M4B " => Some("audio"),
            _ => Some("video"),
        };
    }
    if at(0, &[0x1a, 0x45, 0xdf, 0xa3]) {
        return if ext == "webm" { Some("video") } else { None };
    }
    if at(0, b"OggS") {
        return Some(if ext == "ogv" { "video" } else { "audio" });
    }
    if at(0, b"fLaC") || at(0, b"ID3") || head.len() >= 2 && head[0] == 0xff && head[1] & 0xe0 == 0xe0 {
        return Some("audio");
    }
    if head.windows(5).take(1024).any(|w| w == b"%PDF-") {
        return Some("pdf");
    }
    if ext == "svg" && looks_text(head) && String::from_utf8_lossy(head).contains("<svg") {
        return Some("image");
    }
    None
}

/// Plain text (UTF-8, or UTF-16 with a byte-order mark).
pub fn looks_text(head: &[u8]) -> bool {
    if head.starts_with(&[0xff, 0xfe]) || head.starts_with(&[0xfe, 0xff]) {
        return true;
    }
    if head.contains(&0) {
        return false;
    }
    let s = String::from_utf8_lossy(head);
    let bad = s.chars().filter(|c| *c == '\u{fffd}' || (c.is_control() && !c.is_whitespace())).count();
    bad * 50 <= s.chars().count().max(1)
}

/// A PDF that ends without its `%%EOF` trailer is truncated or damaged.
fn pdf_complete(path: &Path, len: u64) -> bool {
    use std::io::{Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let tail = len.min(2048);
    if f.seek(SeekFrom::Start(len - tail)).is_err() {
        return false;
    }
    let mut buf = Vec::with_capacity(tail as usize);
    f.take(tail).read_to_end(&mut buf).is_ok() && buf.windows(5).any(|w| w == b"%%EOF")
}

pub fn probe(path: &str) -> Probe {
    let p = Path::new(path);
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let ext = ext_of(&name);
    let none = |exists: bool, is_dir: bool, size: i64, kind: &'static str, note: String| Probe {
        exists,
        is_dir,
        size,
        mode: "none",
        kind,
        note: Some(note),
    };
    let meta = match std::fs::metadata(p) {
        Ok(m) => m,
        Err(e) => return none(false, false, 0, crate::model::kind_of(&ext), friendly_io(&e)),
    };
    if meta.is_dir() {
        return Probe { exists: true, is_dir: true, size: 0, mode: "none", kind: "folder", note: None };
    }
    let size = meta.len() as i64;
    let kind = crate::classify::kind_for(p, &ext);
    if platform::attrs(&meta) & platform::ATTR_CLOUD != 0 {
        return none(true, false, size, kind, "This file is stored online only. Open it to download a copy.".into());
    }
    if size == 0 {
        return none(true, false, 0, kind, "This file is empty.".into());
    }
    let head = match crate::classify::read_head(p, crate::classify::HEAD) {
        Ok(h) => h,
        Err(e) => return none(true, false, size, kind, friendly_io(&e)),
    };
    let expected = expected_mode(&ext);
    let sniffed = sniff_media(&head, &ext);
    let label = if ext.is_empty() { "its name".to_string() } else { format!("a .{ext} file") };
    let mode = match (expected, sniffed) {
        ("image", Some("image")) if meta.len() > MAX_IMAGE_BYTES => {
            return none(true, false, size, kind, "This image is too large to preview here.".into())
        }
        ("pdf", Some("pdf")) if !pdf_complete(p, meta.len()) => {
            return none(true, false, size, kind, "This PDF looks damaged or incomplete, so there's no preview.".into())
        }
        (e, Some(s)) if e == s => s,
        ("text", _) | ("none", None) if looks_text(&head) => "text",
        ("none", Some(s)) => s, // e.g. a PNG saved without an extension
        ("none", None) => {
            let note = if kind == "object" {
                "Compiled object code from a build (not a 3D model). No preview.".to_string()
            } else {
                "No preview for this kind of file.".to_string()
            };
            return none(true, false, size, kind, note);
        }
        ("text", _) => return none(true, false, size, kind, "This file contains binary data, so there's no text preview.".into()),
        _ => return none(true, false, size, kind, format!("The contents don't match what {label} should contain, so there's no preview.")),
    };
    Probe { exists: true, is_dir: false, size, mode, kind, note: None }
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    #[test]
    fn hostile_files_degrade_to_a_note_and_never_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let w = |name: &str, data: &[u8]| {
            let p = tmp.path().join(name);
            std::fs::write(&p, data).unwrap();
            p.to_string_lossy().to_string()
        };
        let png = w("real.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
        let fake_png = w("fake.png", b"<html>not a png</html>");
        let pdf = w("doc.pdf", b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n");
        let truncated_pdf = w("truncated.pdf", b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n");
        let broken_pdf = w("broken.pdf", &[0x13; 3000]);
        let binary_txt = w("binary.txt", &[0, 159, 146, 150, 0, 1, 2]);
        let text = w("notes.md", b"# Notes\nhello");
        let utf16 = w("u16.txt", &[0xff, 0xfe, b'h', 0, b'i', 0]);
        let empty = w("empty.jpg", b"");
        let msvc = w("flag_check.obj", &[0x64, 0x86, 4, 0, 0xd8, 0xfd, 0x32, 0xef, 0xbe, 1, 0, 0, 12, 0, 0, 0, 0, 0, 0, 0, b'.', b'd']);
        let wavefront = w("cube.obj", b"o Cube\nv 1 1 1\nv 1 0 1\nf 1 2 1\n");
        let mp4 = w("clip.mp4", b"\0\0\0\x18ftypmp42\0\0\0\0");
        let fake_mp4 = w("fake.mp4", b"just text");
        let png_no_ext = w("thumbnail", b"\x89PNG\r\n\x1a\n\0\0");
        let svg = w("logo.svg", b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"></svg>");

        let m = |p: &str| probe(p).mode;
        assert_eq!(m(&png), "image");
        assert_eq!(m(&pdf), "pdf");
        assert_eq!(m(&text), "text");
        assert_eq!(m(&utf16), "text");
        assert_eq!(m(&mp4), "video");
        assert_eq!(m(&png_no_ext), "image");
        assert_eq!(m(&svg), "image");
        assert_eq!(m(&wavefront), "text", "a Wavefront model is readable text");
        for p in [&fake_png, &broken_pdf, &truncated_pdf, &binary_txt, &empty, &fake_mp4, &msvc] {
            let r = probe(p);
            assert_eq!(r.mode, "none", "{p}");
            assert!(r.note.is_some(), "{p} needs an explanation");
        }
        assert_eq!(probe(&msvc).kind, "object");
        assert!(probe(&msvc).note.unwrap().contains("not a 3D model"));
        assert_eq!(probe(&wavefront).kind, "cad");

        // A file that disappears before (or while) we look.
        std::fs::remove_file(&text).unwrap();
        let gone = probe(&text);
        assert!(!gone.exists);
        assert_eq!(gone.note.as_deref(), Some("This file was moved or deleted."));
        assert!(read_text(&text, 1024).unwrap_err().to_string().contains("moved or deleted"));
    }

    #[cfg(windows)]
    #[test]
    fn locked_files_get_a_friendly_note() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("locked.txt");
        std::fs::write(&p, b"hello").unwrap();
        let _hold = std::fs::OpenOptions::new().read(true).share_mode(0).open(&p).unwrap();
        let r = probe(&p.to_string_lossy());
        assert_eq!(r.mode, "none");
        assert_eq!(r.note.as_deref(), Some("Another app is using this file right now."));
    }
}
