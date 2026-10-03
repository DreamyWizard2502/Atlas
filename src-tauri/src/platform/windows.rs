use super::{DiskSpace, ATTR_CLOUD, ATTR_HIDDEN, ATTR_SYSTEM};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x40000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x400000;
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn attrs(meta: &std::fs::Metadata) -> u32 {
    let a = meta.file_attributes();
    let mut out = 0;
    if a & FILE_ATTRIBUTE_HIDDEN != 0 {
        out |= ATTR_HIDDEN;
    }
    if a & FILE_ATTRIBUTE_SYSTEM != 0 {
        out |= ATTR_SYSTEM;
    }
    if a & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0 {
        out |= ATTR_CLOUD;
    }
    out
}

pub fn volume_root(path: &Path) -> Option<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW;
    let w = wide(path.as_os_str());
    let mut buf = vec![0u16; 1024];
    let ok = unsafe { GetVolumePathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if ok == 0 {
        // Fall back to the path prefix (e.g. "C:\").
        return path.ancestors().last().map(|p| p.to_path_buf()).or_else(|| {
            path.components().next().map(|c| PathBuf::from(c.as_os_str()).join("\\"))
        });
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
}

pub fn disk_space(path: &Path) -> Option<DiskSpace> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let root = volume_root(path)?;
    let w = wide(root.as_os_str());
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) };
    if ok == 0 {
        return None;
    }
    Some(DiskSpace { mount: root.to_string_lossy().to_string(), total, free })
}

pub fn is_locked(path: &Path) -> bool {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if meta.is_dir() {
        return false;
    }
    // Opening with no sharing fails if any other process has the file open.
    match std::fs::OpenOptions::new().read(true).share_mode(0).open(path) {
        Ok(_) => false,
        Err(e) => matches!(e.raw_os_error(), Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION)),
    }
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

pub fn system_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    out.push(env_dir("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows")));
    out.push(env_dir("ProgramFiles").unwrap_or_else(|| PathBuf::from(r"C:\Program Files")));
    out.push(env_dir("ProgramFiles(x86)").unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)")));
    out.push(env_dir("ProgramData").unwrap_or_else(|| PathBuf::from(r"C:\ProgramData")));
    // Always include the literal defaults too, in case env vars point elsewhere.
    for p in [r"C:\Windows", r"C:\Program Files", r"C:\Program Files (x86)", r"C:\ProgramData"] {
        let p = PathBuf::from(p);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

pub fn system_temp_dir() -> Option<PathBuf> {
    Some(env_dir("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("Temp"))
}

pub fn installed_app_names() -> Vec<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let mut names = Vec::new();
    let locations = [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];
    for (hive, path) in locations {
        let Ok(key) = RegKey::predef(hive).open_subkey(path) else { continue };
        for sub in key.enum_keys().flatten() {
            if let Ok(k) = key.open_subkey(&sub) {
                if let Ok(name) = k.get_value::<String, _>("DisplayName") {
                    names.push(name);
                }
            }
        }
    }
    names
}

/// Call GetLongPathNameW / GetShortPathNameW style APIs with a sized buffer.
fn path_api(path: &Path, f: unsafe extern "system" fn(*const u16, *mut u16, u32) -> u32) -> Option<PathBuf> {
    let w = wide(path.as_os_str());
    let needed = unsafe { f(w.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }
    let mut buf = vec![0u16; needed as usize];
    let len = unsafe { f(w.as_ptr(), buf.as_mut_ptr(), needed) };
    if len == 0 || len >= needed {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buf[..len as usize])))
}

pub fn long_path(path: &Path) -> PathBuf {
    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;
    if let Some(p) = path_api(path, GetLongPathNameW) {
        return p;
    }
    // Deleted files can't be resolved; expand the parent and keep the name.
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Some(p) = path_api(parent, GetLongPathNameW) {
            return p.join(name);
        }
    }
    path.to_path_buf()
}

pub fn short_path(path: &Path) -> Option<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    path_api(path, GetShortPathNameW)
}

pub fn list_roots() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|c| PathBuf::from(format!("{}:\\", c as char)))
        .filter(|p| std::fs::metadata(p).is_ok())
        .collect()
}
