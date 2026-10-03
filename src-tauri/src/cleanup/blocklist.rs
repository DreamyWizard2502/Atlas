//! Hard blocklist. Cleanup refuses to touch anything matching these rules, both
//! at analysis time and again immediately before moving each item.

use crate::platform;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Blocklist {
    /// OS and program install directories (Windows, Program Files, ProgramData).
    pub system_roots: Vec<PathBuf>,
    /// Paths inside a system root that are nonetheless allowed (Windows\Temp).
    pub system_exceptions: Vec<PathBuf>,
    /// The roaming profile (AppData\Roaming). Blocked except `roaming_allow`.
    pub roaming: Option<PathBuf>,
    pub roaming_allow: Vec<PathBuf>,
    /// Atlas's own data and holding areas.
    pub own_dirs: Vec<PathBuf>,
    /// Volume roots — never move a drive itself.
    pub volume_roots: Vec<PathBuf>,
}

impl Blocklist {
    /// The real blocklist for this machine.
    pub fn for_system(own_dirs: Vec<PathBuf>) -> Self {
        let roaming = platform::roaming_dir();
        let roaming_allow = roaming
            .as_ref()
            .map(|r| platform::roaming_cache_rel().iter().map(|rel| r.join(rel)).collect())
            .unwrap_or_default();
        Blocklist {
            system_roots: platform::system_roots(),
            system_exceptions: platform::system_temp_dir().into_iter().collect(),
            roaming,
            roaming_allow,
            own_dirs,
            volume_roots: platform::list_roots(),
        }
    }

    /// Pure path rules (no disk access). Returns the reason if blocked.
    pub fn check_path(&self, path: &Path) -> Result<(), String> {
        if path.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")) {
            return Err("Inside a .git folder".into());
        }
        if self.volume_roots.iter().any(|v| platform::norm(v) == platform::norm(path)) || path.parent().is_none() {
            return Err("Drive root".into());
        }
        if self.own_dirs.iter().any(|d| platform::is_within(path, d)) {
            return Err("Atlas data or holding area".into());
        }
        let excepted = self.system_exceptions.iter().any(|e| platform::is_within(path, e) && platform::norm(path) != platform::norm(e));
        if !excepted {
            if let Some(root) = self.system_roots.iter().find(|r| platform::is_within(path, r)) {
                return Err(format!("Protected system location ({})", root.display()));
            }
        }
        if let Some(roaming) = &self.roaming {
            if platform::is_within(path, roaming) {
                let allowed = self
                    .roaming_allow
                    .iter()
                    .any(|a| platform::is_within(path, a) && platform::norm(path) != platform::norm(a));
                if !allowed {
                    return Err("AppData\\Roaming (not a known cache)".into());
                }
            }
        }
        Ok(())
    }

    /// Full live check: path rules, system attributes, and open/locked files.
    pub fn check_live(&self, path: &Path) -> Result<(), String> {
        self.check_path(path)?;
        let meta = std::fs::symlink_metadata(path).map_err(|_| "No longer exists".to_string())?;
        if meta.file_type().is_symlink() {
            return Err("Symbolic link or junction".into());
        }
        if platform::is_system(platform::attrs(&meta)) {
            return Err("System file".into());
        }
        if platform::is_locked(path) {
            return Err("File is open in another app".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tmp: &Path) -> Blocklist {
        let win = tmp.join("Windows");
        Blocklist {
            system_roots: vec![win.clone(), tmp.join("Program Files"), tmp.join("Program Files (x86)"), tmp.join("ProgramData")],
            system_exceptions: vec![win.join("Temp")],
            roaming: Some(tmp.join("Users/me/AppData/Roaming")),
            roaming_allow: vec![tmp.join("Users/me/AppData/Roaming/Code/Cache")],
            own_dirs: vec![tmp.join("Atlas/holding")],
            volume_roots: vec![],
        }
    }

    #[test]
    fn blocks_system_locations() {
        let tmp = tempfile::tempdir().unwrap();
        let b = fixture(tmp.path());
        assert!(b.check_path(&tmp.path().join("Windows/System32/kernel32.dll")).is_err());
        assert!(b.check_path(&tmp.path().join("Program Files/App/app.exe")).is_err());
        assert!(b.check_path(&tmp.path().join("Program Files (x86)/App/app.exe")).is_err());
        assert!(b.check_path(&tmp.path().join("ProgramData/x.dat")).is_err());
        // Case-insensitive on Windows.
        if cfg!(windows) {
            assert!(b.check_path(&tmp.path().join("WINDOWS/notepad.exe")).is_err());
        }
        // A sibling that merely shares a prefix is fine.
        assert!(b.check_path(&tmp.path().join("WindowsApps-notes/a.txt")).is_ok());
    }

    #[test]
    fn windows_temp_is_the_only_exception() {
        let tmp = tempfile::tempdir().unwrap();
        let b = fixture(tmp.path());
        assert!(b.check_path(&tmp.path().join("Windows/Temp/old.log")).is_ok());
        // The temp folder itself can't be moved.
        assert!(b.check_path(&tmp.path().join("Windows/Temp")).is_err());
        assert!(b.check_path(&tmp.path().join("Windows/TempX/a")).is_err());
    }

    #[test]
    fn roaming_blocked_except_known_caches() {
        let tmp = tempfile::tempdir().unwrap();
        let b = fixture(tmp.path());
        let roaming = tmp.path().join("Users/me/AppData/Roaming");
        assert!(b.check_path(&roaming.join("Mozilla/profile/key4.db")).is_err());
        assert!(b.check_path(&roaming.join("Code/Cache/data_1")).is_ok());
        assert!(b.check_path(&roaming.join("Code/User/settings.json")).is_err());
    }

    #[test]
    fn git_and_own_dirs_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let b = fixture(tmp.path());
        assert!(b.check_path(&tmp.path().join("proj/.git/objects/ab/cdef")).is_err());
        assert!(b.check_path(&tmp.path().join("proj/.GIT/HEAD")).is_err());
        assert!(b.check_path(&tmp.path().join("proj/.github/workflow.yml")).is_ok());
        assert!(b.check_path(&tmp.path().join("Atlas/holding/b1/1/file.txt")).is_err());
        assert!(b.check_path(&tmp.path().join("proj/src/main.rs")).is_ok());
    }

    #[test]
    fn live_check_rejects_missing_and_locked() {
        let tmp = tempfile::tempdir().unwrap();
        let b = fixture(tmp.path());
        assert!(b.check_live(&tmp.path().join("nope.txt")).is_err());
        let f = tmp.path().join("open.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(b.check_live(&f).is_ok());
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let _held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&f).unwrap();
            assert_eq!(b.check_live(&f).unwrap_err(), "File is open in another app");
        }
    }
}
