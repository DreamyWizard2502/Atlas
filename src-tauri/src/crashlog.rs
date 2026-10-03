//! Persistent crash and error log, so diagnosing a crash never depends on a
//! terminal being open.
//!
//! - A panic hook writes the message, location, thread and backtrace.
//! - The frontend's error boundaries (and uncaught JS errors) write through
//!   the `log_client_error` command.
//! - Native crashes (e.g. a stack overflow) bypass the panic hook, so each
//!   launch leaves a session marker; if the previous one wasn't closed
//!   cleanly, the next launch records that it ended unexpectedly.
//!
//! Files live in `<data dir>/logs`: `atlas.log` rolls over at `max_bytes` to
//! `atlas.1.log`, `atlas.2.log`, ... keeping `keep` old files.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const LOG_NAME: &str = "atlas.log";
const SESSION_MARKER: &str = "session.running";
pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024;
pub const DEFAULT_KEEP: usize = 3;

#[derive(Debug)]
pub struct CrashLog {
    dir: PathBuf,
    max_bytes: u64,
    keep: usize,
    write_lock: Mutex<()>,
}

/// The log the panic hook and commands write to.
static GLOBAL: Mutex<Option<Arc<CrashLog>>> = Mutex::new(None);

impl CrashLog {
    pub fn new(dir: PathBuf, max_bytes: u64, keep: usize) -> Self {
        CrashLog { dir, max_bytes, keep, write_lock: Mutex::new(()) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(LOG_NAME)
    }

    fn rotated(&self, n: usize) -> PathBuf {
        self.dir.join(format!("atlas.{n}.log"))
    }

    /// Shift atlas.log -> atlas.1.log -> ... dropping the oldest.
    fn rotate(&self) -> std::io::Result<()> {
        if self.keep == 0 {
            return fs::remove_file(self.path());
        }
        let _ = fs::remove_file(self.rotated(self.keep));
        for n in (1..self.keep).rev() {
            let from = self.rotated(n);
            if from.exists() {
                fs::rename(&from, self.rotated(n + 1))?;
            }
        }
        fs::rename(self.path(), self.rotated(1))
    }

    /// Append one entry. Never panics: logging must not make things worse.
    pub fn write(&self, level: &str, source: &str, body: &str) -> std::io::Result<()> {
        // A poisoned lock just means an earlier writer panicked; keep going.
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        fs::create_dir_all(&self.dir)?;
        let entry = format!(
            "=== {} | {level} | {source} | Atlas {} (pid {})\n{}\n\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f %:z"),
            env!("CARGO_PKG_VERSION"),
            std::process::id(),
            body.trim_end()
        );
        let current = fs::metadata(self.path()).map(|m| m.len()).unwrap_or(0);
        if current > 0 && current + entry.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        let mut f = OpenOptions::new().create(true).append(true).open(self.path())?;
        // An entry larger than the cap is cut, not dropped.
        let bytes = entry.as_bytes();
        let cap = self.max_bytes.max(1) as usize;
        if bytes.len() > cap {
            f.write_all(&bytes[..cap.saturating_sub(32)])?;
            f.write_all(b"\n[entry truncated]\n\n")?;
        } else {
            f.write_all(bytes)?;
        }
        f.flush()
    }

    /// Start of a session: report an unclean previous exit, then mark this one.
    /// Returns the previous session's marker contents if it never closed.
    pub fn begin_session(&self) -> Option<String> {
        let marker = self.dir.join(SESSION_MARKER);
        let previous = fs::read_to_string(&marker).ok();
        if let Some(prev) = &previous {
            let _ = self.write(
                "WARN",
                "session",
                &format!(
                    "The previous session did not close cleanly ({}).\nIf Atlas crashed without a panic entry above, it was a native crash (for example a stack overflow); Windows records those in Event Viewer > Windows Logs > Application.",
                    prev.trim()
                ),
            );
        }
        let _ = fs::create_dir_all(&self.dir);
        let _ = fs::write(
            &marker,
            format!("started {} (pid {})", chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z"), std::process::id()),
        );
        previous
    }

    /// Clean exit: remove the session marker.
    pub fn end_session(&self) {
        let _ = fs::remove_file(self.dir.join(SESSION_MARKER));
    }
}

/// Make `log` the target for the panic hook and `write_event`.
pub fn set_global(log: Option<Arc<CrashLog>>) {
    *GLOBAL.lock().unwrap_or_else(|e| e.into_inner()) = log;
}

pub fn global() -> Option<Arc<CrashLog>> {
    GLOBAL.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Write through the global log (no-op before `install`).
pub fn write_event(level: &str, source: &str, body: &str) {
    if let Some(log) = global() {
        let _ = log.write(level, source, body);
    }
}

/// Text of a panic: message, location, thread and a forced backtrace.
pub fn describe_panic(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(non-string panic payload)".to_string());
    let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_else(|| "unknown location".into());
    let thread = std::thread::current().name().map(str::to_string).unwrap_or_else(|| "unnamed".into());
    let backtrace = std::backtrace::Backtrace::force_capture();
    format!("thread '{thread}' panicked at {location}:\n{message}\n\nbacktrace:\n{backtrace}")
}

/// Install the panic hook (chained before the default one, so the terminal
/// still shows panics when there is one) and make `log` the global target.
pub fn install(log: Arc<CrashLog>) {
    set_global(Some(log));
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_event("PANIC", "rust", &describe_panic(info));
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that touch the global log or the process-wide panic hook take this.
    static GLOBAL_TEST: Mutex<()> = Mutex::new(());

    fn read_all(dir: &Path) -> String {
        let mut names: Vec<_> = fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        names.sort();
        names.iter().map(|p| fs::read_to_string(p).unwrap_or_default()).collect()
    }

    #[test]
    fn rolls_over_at_the_cap_and_keeps_a_fixed_number_of_files() {
        let tmp = tempfile::tempdir().unwrap();
        let log = CrashLog::new(tmp.path().join("logs"), 2_000, 2);
        for i in 0..60 {
            log.write("ERROR", "test", &format!("entry {i:03} {}", "x".repeat(80))).unwrap();
        }
        let mut files: Vec<String> = fs::read_dir(log.dir()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        files.sort();
        assert_eq!(files, vec!["atlas.1.log", "atlas.2.log", "atlas.log"]);
        for f in &files {
            assert!(fs::metadata(log.dir().join(f)).unwrap().len() <= 2_000, "{f} exceeds the cap");
        }
        // Newest entry is in atlas.log; the oldest ones were dropped.
        assert!(fs::read_to_string(log.path()).unwrap().contains("entry 059"));
        assert!(!read_all(log.dir()).contains("entry 000"));
    }

    #[test]
    fn oversized_entries_are_truncated_not_lost() {
        let tmp = tempfile::tempdir().unwrap();
        let log = CrashLog::new(tmp.path().to_path_buf(), 500, 1);
        log.write("ERROR", "test", &"y".repeat(5_000)).unwrap();
        let text = fs::read_to_string(log.path()).unwrap();
        assert!(text.len() <= 520 && text.contains("[entry truncated]"));
    }

    #[test]
    fn panic_hook_writes_message_location_and_backtrace() {
        let _g = GLOBAL_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let log = Arc::new(CrashLog::new(tmp.path().to_path_buf(), DEFAULT_MAX_BYTES, DEFAULT_KEEP));
        let before = std::panic::take_hook();
        install(log.clone());
        let line = line!() + 2;
        let result = std::thread::Builder::new().name("crash-test".into()).spawn(|| {
            panic!("boom from a test thread");
        });
        let _ = result.unwrap().join();
        // Restore the process-wide state for other tests.
        set_global(None);
        std::panic::set_hook(before);

        let text = fs::read_to_string(log.path()).unwrap();
        assert!(text.contains("PANIC | rust"), "{text}");
        assert!(text.contains("boom from a test thread"));
        assert!(text.contains("thread 'crash-test' panicked at"));
        assert!(text.contains(&format!("crashlog.rs:{line}")), "{text}");
        assert!(text.contains("backtrace:"));
    }

    #[test]
    fn unclean_previous_session_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let log = CrashLog::new(tmp.path().to_path_buf(), DEFAULT_MAX_BYTES, DEFAULT_KEEP);
        assert_eq!(log.begin_session(), None, "first launch");
        log.end_session();
        assert_eq!(log.begin_session(), None, "clean exit last time");
        // Simulate a crash: the marker is never removed.
        let prev = log.begin_session();
        assert!(prev.unwrap().starts_with("started "));
        assert!(fs::read_to_string(log.path()).unwrap().contains("did not close cleanly"));
    }

    #[test]
    fn write_event_without_a_log_is_a_no_op() {
        let _g = GLOBAL_TEST.lock().unwrap_or_else(|e| e.into_inner());
        set_global(None);
        write_event("ERROR", "test", "nowhere to go");
    }
}
