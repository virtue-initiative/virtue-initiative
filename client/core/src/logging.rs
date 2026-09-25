//! Rotation/retention conventions shared by every platform that installs a
//! `tracing` subscriber. `core` never installs a subscriber itself (see
//! `client/CLAUDE.md`'s cross-component contracts) — only the host process
//! knows its own process model — but the naming/retention knobs are pure
//! logic worth centralizing rather than reimplementing per platform.

pub fn log_error(msg: &str, err: Option<&dyn std::fmt::Display>) {
    match err {
        Some(e) => tracing::error!(error = %e, "{msg}"),
        None => tracing::error!("{msg}"),
    }
}

pub fn log_warning(msg: &str, err: Option<&dyn std::fmt::Display>) {
    match err {
        Some(e) => tracing::warn!(error = %e, "{msg}"),
        None => tracing::warn!("{msg}"),
    }
}

/// Default `EnvFilter` directive — the fallback when no runtime override
/// (`RUST_LOG`, where supported) is present. Always debug-level for
/// `virtue_core` so release builds retain the same diagnostic detail as
/// debug builds; `debug_build` is accepted for call-site symmetry with
/// platforms that also gate their own crate's directive on it.
pub fn default_filter_directive(_debug_build: bool) -> &'static str {
    "info,virtue_core=debug"
}

/// Rolling-file naming/retention knobs shared by every platform with a file
/// sink (everything except Linux, which stays on stdout -> journald).
pub struct FileLogPolicy {
    pub file_name_prefix: &'static str,
    pub max_retained_files: usize,
}

pub const DEFAULT_FILE_LOG_POLICY: FileLogPolicy = FileLogPolicy {
    file_name_prefix: "virtue",
    max_retained_files: 14,
};

/// Files in `dir` whose name starts with `policy.file_name_prefix`, newest
/// (by modified time) first.
fn log_files_newest_first(
    dir: &std::path::Path,
    policy: &FileLogPolicy,
) -> std::io::Result<Vec<(std::time::SystemTime, std::path::PathBuf)>> {
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(policy.file_name_prefix))
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    Ok(files)
}

/// How far back `recent_logs` reaches: a bug report's "last day of logs".
const RECENT_LOG_WINDOW: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Best-effort last day of a file-sink platform's logs for a bug report's
/// `log_file` attachment (API-042): every log file in `dir` written to within
/// the past 24 hours, oldest first, redacted (`api::redact_secrets`) and
/// trimmed to `api::MAX_LOG_ATTACHMENT_BYTES`, keeping the most recent bytes.
/// Daily rotation means that is normally two files, so an issue right around
/// the rollover keeps the lines on both sides of it. If nothing was written
/// in that window (the daemon hasn't run for a day), the newest file is sent
/// instead, since how it stopped is exactly what the report needs. `None` if
/// there are no log files or they are all empty.
///
/// Picks files by modified time rather than by reconstructing their dated
/// names: `tracing_appender` dates files in UTC, so a local-date lookup misses
/// the current file whenever the two dates differ (every US evening), and a
/// name lookup also breaks on any prefix/suffix mismatch with the writer.
pub fn recent_logs(dir: &std::path::Path) -> Option<Vec<u8>> {
    recent_logs_at(dir, std::time::SystemTime::now())
}

fn recent_logs_at(dir: &std::path::Path, now: std::time::SystemTime) -> Option<Vec<u8>> {
    let files = log_files_newest_first(dir, &DEFAULT_FILE_LOG_POLICY).ok()?;
    let cutoff = now.checked_sub(RECENT_LOG_WINDOW)?;
    let in_window = files
        .iter()
        .take_while(|(modified, _)| *modified >= cutoff)
        .count()
        .max(1);

    let mut combined = String::new();
    for (_, path) in files.iter().take(in_window).rev() {
        if let Ok(contents) = std::fs::read(path) {
            combined.push_str(&String::from_utf8_lossy(&contents));
        }
    }

    if combined.is_empty() {
        return None;
    }

    let redacted = crate::api::redact_secrets(&combined);
    let mut logs = redacted.into_bytes();
    if logs.len() > crate::api::MAX_LOG_ATTACHMENT_BYTES {
        let start = logs.len() - crate::api::MAX_LOG_ATTACHMENT_BYTES;
        logs.drain(0..start);
    }
    Some(logs)
}

/// Deletes all but the newest `max_retained_files` files in `dir` whose name
/// starts with `policy.file_name_prefix`, sorted by modified time. Call once
/// at subscriber-install time on platforms using a file sink —
/// `tracing_appender` itself does not prune old files.
pub fn prune_old_logs(dir: &std::path::Path, policy: &FileLogPolicy) -> std::io::Result<()> {
    let files = log_files_newest_first(dir, policy)?;
    if files.len() <= policy.max_retained_files {
        return Ok(());
    }

    for (_, path) in files.into_iter().skip(policy.max_retained_files) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_directive_is_debug_regardless_of_build_type() {
        assert_eq!(default_filter_directive(true), "info,virtue_core=debug");
        assert_eq!(default_filter_directive(false), "info,virtue_core=debug");
    }

    #[test]
    fn prune_old_logs_keeps_only_newest_files() {
        let dir = std::env::temp_dir().join(format!(
            "virtue-logging-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let policy = FileLogPolicy {
            file_name_prefix: "virtue",
            max_retained_files: 2,
        };

        // Create 4 matching files plus one non-matching file, with distinct
        // mtimes (some filesystems have coarse mtime resolution, so bump the
        // clock explicitly rather than relying on real elapsed time).
        let names = [
            "virtue.2024-01-01",
            "virtue.2024-01-02",
            "virtue.2024-01-03",
            "virtue.2024-01-04",
        ];
        for (i, name) in names.iter().enumerate() {
            let path = dir.join(name);
            std::fs::write(&path, b"x").unwrap();
            let mtime = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_700_000_000 + i as u64 * 3600);
            // `File::open` is read-only, which lacks the write-attributes
            // permission `set_modified` needs on Windows.
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.set_modified(mtime).unwrap();
        }
        std::fs::write(dir.join("other.log"), b"x").unwrap();

        prune_old_logs(&dir, &policy).unwrap();

        let remaining: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert!(remaining.contains(&"virtue.2024-01-03".to_string()));
        assert!(remaining.contains(&"virtue.2024-01-04".to_string()));
        assert!(!remaining.contains(&"virtue.2024-01-01".to_string()));
        assert!(!remaining.contains(&"virtue.2024-01-02".to_string()));
        assert!(
            remaining.contains(&"other.log".to_string()),
            "non-matching files are untouched"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn temp_log_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "virtue-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes `contents` to `dir/name` with a modified time `hours_ago`
    /// hours before `now`.
    fn write_log(
        dir: &std::path::Path,
        name: &str,
        contents: &str,
        now: std::time::SystemTime,
        hours_ago: u64,
    ) {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        let mtime = now - std::time::Duration::from_secs(hours_ago * 3600);
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(mtime).unwrap();
    }

    #[test]
    fn recent_logs_includes_every_file_written_in_the_past_day_oldest_first() {
        let dir = temp_log_dir("recent-logs-window");
        let now = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        assert!(
            recent_logs_at(&dir, now).is_none(),
            "no files -> no attachment"
        );

        // Mixed naming (Android's pre-fix extensionless files next to `.log`
        // ones) and far-future dates (UTC naming vs. the local clock) must
        // not matter; only modified time does.
        write_log(&dir, "virtue.2024-01-01", "too old\n", now, 30);
        write_log(
            &dir,
            "virtue.2024-01-02",
            "yesterday wst_AbCdEf123456ghijklmnop\n",
            now,
            20,
        );
        write_log(&dir, "virtue.2099-01-02.log", "after upgrade\n", now, 10);
        write_log(&dir, "virtue.2099-01-03.log", "today\n", now, 1);
        write_log(&dir, "other.log", "unrelated\n", now, 0);

        let text = String::from_utf8(recent_logs_at(&dir, now).unwrap()).unwrap();
        assert_eq!(text, "yesterday [redacted]\nafter upgrade\ntoday\n");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recent_logs_falls_back_to_the_newest_file_when_nothing_is_recent() {
        let dir = temp_log_dir("recent-logs-stale");
        let now = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        write_log(&dir, "virtue.2024-01-01.log", "older\n", now, 72);
        write_log(&dir, "virtue.2024-01-02.log", "last words\n", now, 48);

        let text = String::from_utf8(recent_logs_at(&dir, now).unwrap()).unwrap();
        assert_eq!(text, "last words\n");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
