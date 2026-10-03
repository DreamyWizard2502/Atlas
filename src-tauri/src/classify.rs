//! What a file really is. Extensions can lie (an MSVC `.obj` is compiled object
//! code, a Wavefront `.obj` is a 3D model), so ambiguous extensions are
//! confirmed from the first bytes of the file. Also assigns each file one of the
//! storage categories used to color the treemap.

use crate::model::kind_of;
use std::io::Read;
use std::path::Path;

/// How many leading bytes are inspected when sniffing.
pub const HEAD: usize = 4096;

/// Storage categories, in a fixed order (index = `files.cat`, `files.cbN`).
pub const CATEGORIES: [(&str, &str); 8] = [
    ("games", "Games and apps"),
    ("cad", "CAD and 3D"),
    ("documents", "Documents"),
    ("media", "Media"),
    ("code", "Code and projects"),
    ("archives", "Archives and installers"),
    ("temp", "Temp and cache"),
    ("other", "Other"),
];
pub const GAMES: u8 = 0;
pub const CAD: u8 = 1;
pub const DOCUMENTS: u8 = 2;
pub const MEDIA: u8 = 3;
pub const CODE: u8 = 4;
pub const ARCHIVES: u8 = 5;
pub const TEMP: u8 = 6;
pub const OTHER: u8 = 7;

pub fn read_head(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(n);
    f.take(n as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Compiled object code: COFF (MSVC .obj), anonymous/LTCG objects, ELF, Mach-O.
pub fn is_object_code(head: &[u8]) -> bool {
    const COFF_MACHINES: [[u8; 2]; 5] = [
        [0x64, 0x86], // x64
        [0x4c, 0x01], // x86
        [0x64, 0xaa], // ARM64
        [0xc4, 0x01], // ARMv7
        [0x41, 0xa6], // ARM64EC
    ];
    head.len() >= 20 && COFF_MACHINES.iter().any(|m| head[..2] == m[..])
        || head.starts_with(&[0x00, 0x00, 0xff, 0xff])
        || head.starts_with(b"\x7fELF")
        || head.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
        || head.starts_with(&[0xce, 0xfa, 0xed, 0xfe])
}

/// Text that is overwhelmingly Wavefront OBJ statements (`v`, `vn`, `f`...).
pub fn looks_like_wavefront(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    let text = String::from_utf8_lossy(head);
    let (mut obj, mut other) = (0usize, 0usize);
    for line in text.lines().take(400) {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        match t.split_whitespace().next().unwrap_or("") {
            "v" | "vn" | "vt" | "vp" | "f" | "l" | "p" | "o" | "g" | "s" | "mtllib" | "usemtl" | "cstype" | "deg" => obj += 1,
            _ => other += 1,
        }
    }
    obj >= 1 && obj >= other * 4
}

/// The kind for a file, confirmed from content when the extension is ambiguous.
/// Only ambiguous extensions are read; everything else is decided by name.
pub fn kind_for(path: &Path, ext: &str) -> &'static str {
    match ext {
        "obj" => match read_head(path, HEAD) {
            Ok(head) if looks_like_wavefront(&head) => "cad",
            Ok(head) if is_object_code(&head) => "object",
            // Unreadable, vanished or unrecognised: don't claim it's a 3D model.
            _ => "other",
        },
        _ => kind_of(ext),
    }
}

/// Folders where programs and games are installed. Paths are lowercase.
pub const INSTALL_MARKERS: &[&str] = &[
    "\\program files\\",
    "\\program files (x86)\\",
    "\\windowsapps\\",
    "\\steamapps\\",
    "\\steamlibrary\\",
    "\\steam\\steamapps\\",
    "\\epic games\\",
    "\\gog galaxy\\games\\",
    "\\gog games\\",
    "\\origin games\\",
    "\\ea games\\",
    "\\ubisoft game launcher\\games\\",
    "\\battle.net\\",
    "\\riot games\\",
    "\\xboxgames\\",
    "\\amazon games\\",
    "\\rockstar games\\",
    "\\itch\\apps\\",
];

const CACHE_MARKERS: &[&str] = &[
    "\\temp\\", "\\tmp\\", "\\cache\\", "\\caches\\", "\\.cache\\", "\\inetcache\\", "\\code cache\\", "\\gpucache\\",
    "\\shadercache\\", "\\crashdumps\\", "\\$recycle.bin\\",
];

const BUILD_MARKERS: &[&str] = &["\\target\\debug\\", "\\target\\release\\", "\\node_modules\\", "\\.git\\"];

pub fn in_install_dir(path_lower: &str) -> bool {
    INSTALL_MARKERS.iter().any(|m| path_lower.contains(m))
}

/// Storage category for a file. Location wins over type: assets inside a game
/// install are game data, files in cache folders are cache.
pub fn category(path_lower: &str, name_lower: &str, ext: &str, kind: &str) -> u8 {
    if in_install_dir(path_lower) || path_lower.contains("\\windows\\") {
        return GAMES;
    }
    if CACHE_MARKERS.iter().any(|m| path_lower.contains(m)) || matches!(ext, "tmp" | "temp" | "cache" | "dmp" | "crdownload" | "part")
    {
        return TEMP;
    }
    if BUILD_MARKERS.iter().any(|m| path_lower.contains(m)) {
        return CODE;
    }
    match kind {
        "cad" => CAD,
        "pdf" | "doc" | "sheet" | "slides" | "text" => DOCUMENTS,
        "image" | "video" | "audio" => MEDIA,
        "code" | "object" => CODE,
        "archive" => ARCHIVES,
        "installer" => {
            if ext == "exe" && !["setup", "install", "installer"].iter().any(|w| name_lower.contains(w)) {
                GAMES
            } else {
                ARCHIVES
            }
        }
        _ => match ext {
            "dll" | "sys" | "pak" | "vpk" | "ucas" | "utoc" | "bundle" | "assets" | "resource" | "bnk" | "wem" | "dds" | "bik"
            | "unity3d" | "rpf" | "forge" | "big" => GAMES,
            "pdb" | "ilk" | "rlib" | "rmeta" | "o" | "lib" | "pch" | "ipch" | "exp" | "tlog" => CODE,
            _ => OTHER,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obj_files_are_sniffed_not_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        // First bytes of a real MSVC flag_check.obj produced by the cc crate.
        let msvc = tmp.path().join("flag_check.obj");
        let mut coff = vec![0x64, 0x86, 0x04, 0x00, 0xd8, 0xfd, 0x32, 0xef, 0xbe, 0x01, 0, 0, 0x0c, 0, 0, 0, 0, 0, 0, 0];
        coff.extend_from_slice(b".drectve\0\0\0\0\0\0\0\0");
        std::fs::write(&msvc, &coff).unwrap();
        let wavefront = tmp.path().join("cube.obj");
        std::fs::write(&wavefront, "# Blender\nmtllib cube.mtl\no Cube\nv 1 1 -1\nv 1 -1 -1\nvn 0 1 0\nvt 0 1\nf 1/1/1 2/1/1 3/1/1\n").unwrap();
        let notes = tmp.path().join("notes.obj");
        std::fs::write(&notes, "These are meeting notes, not geometry.\nAgenda\n").unwrap();
        let ltcg = tmp.path().join("ltcg.obj");
        std::fs::write(&ltcg, [0x00, 0x00, 0xff, 0xff, 0x01, 0x00, 0x64, 0x86, 0, 0, 0, 0]).unwrap();

        assert_eq!(kind_for(&msvc, "obj"), "object");
        assert_eq!(kind_for(&ltcg, "obj"), "object");
        assert_eq!(kind_for(&wavefront, "obj"), "cad");
        assert_eq!(kind_for(&notes, "obj"), "other");
        // Vanished files never crash and are never called 3D models.
        assert_eq!(kind_for(&tmp.path().join("gone.obj"), "obj"), "other");
        // Unambiguous extensions are decided without reading.
        assert_eq!(kind_for(&tmp.path().join("missing.pdf"), "pdf"), "pdf");
    }

    #[test]
    fn categories_follow_location_then_type() {
        let c = |p: &str, k: &str| {
            let pl = p.to_lowercase();
            let name = pl.rsplit('\\').next().unwrap().to_string();
            let ext = crate::db::ext_of(&name);
            category(&pl, &name, &ext, k)
        };
        assert_eq!(c(r"D:\SteamLibrary\steamapps\common\Game\data\music.ogg", "audio"), GAMES);
        assert_eq!(c(r"C:\Program Files\App\app.exe", "installer"), GAMES);
        assert_eq!(c(r"C:\Users\me\Downloads\AppSetup.exe", "installer"), ARCHIVES);
        assert_eq!(c(r"C:\Users\me\Desktop\backup.zip", "archive"), ARCHIVES);
        assert_eq!(c(r"C:\Users\me\AppData\Local\Temp\x.bin", "other"), TEMP);
        assert_eq!(c(r"C:\Users\me\proj\target\debug\deps\a.rlib", "other"), CODE);
        assert_eq!(c(r"C:\Users\me\cad\bracket.sldprt", "cad"), CAD);
        assert_eq!(c(r"C:\Users\me\Documents\plan.pdf", "pdf"), DOCUMENTS);
        assert_eq!(c(r"C:\Users\me\Pictures\a.jpg", "image"), MEDIA);
        assert_eq!(c(r"C:\Users\me\proj\build\flag_check.obj", "object"), CODE);
        assert_eq!(c(r"C:\Users\me\misc\thing.xyz", "other"), OTHER);
    }
}
