//! Rotation/retention conventions shared by every platform that installs a
//! `tracing` subscriber. `core` never installs a subscriber itself (see
//! `client/CLAUDE.md`'s cross-component contracts) — only the host process
//! knows its own process model — but the naming/retention knobs are pure
//! logic worth centralizing rather than reimplementing per platform.

/// Default `EnvFilter` directive per build type — the fallback when no
/// runtime override (`RUST_LOG`, where supported) is present.
pub fn default_filter_directive(debug_build: bool) -> &'static str {
    if debug_build {
        "info,virtue_core=debug"
    } else {
        "info"
    }
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

/// Deletes all but the newest `max_retained_files` files in `dir` whose name
/// starts with `policy.file_name_prefix`, sorted by modified time. Call once
/// at subscriber-install time on platforms using a file sink —
/// `tracing_appender` itself does not prune old files.
pub fn prune_old_logs(dir: &std::path::Path, policy: &FileLogPolicy) -> std::io::Result<()> {
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

    if files.len() <= policy.max_retained_files {
        return Ok(());
    }

    // Newest first, so the retained prefix is the newest `max_retained_files`.
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in files.into_iter().skip(policy.max_retained_files) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_filter_directive_differs_by_build_type() {
        assert_eq!(default_filter_directive(true), "info,virtue_core=debug");
        assert_eq!(default_filter_directive(false), "info");
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
            let file = std::fs::File::open(&path).unwrap();
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
}
