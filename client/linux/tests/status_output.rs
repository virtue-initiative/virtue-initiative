//! Runs the real `virtue status` binary against a state file on disk (no
//! daemon), with output piped so it carries no color codes.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct StateDir(PathBuf);

impl StateDir {
    fn signed_in(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("vst-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let state = root.join("state").join("virtue");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            state.join("event_state.json"),
            r#"{
                "auth": {
                    "device_credentials": {"device_id": "dev-123", "refresh_token": "r"},
                    "account_email": "sam@example.org"
                },
                "errors": {"recent": [{"at_ms": 1, "context": "batch_upload", "message": "boom"}]}
            }"#,
        )
        .unwrap();
        StateDir(root)
    }

    fn status(&self, flags: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_virtue"))
            .arg("status")
            .args(flags)
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .output()
            .unwrap();
        assert!(output.status.success(), "virtue status {flags:?} failed");
        String::from_utf8(output.stdout).unwrap()
    }
}

impl Drop for StateDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// CLIENT-020: plain `virtue status` shows only the headline and the summary rows, then
// points to --full.
#[test]
fn status_shows_summary_by_default() {
    let dir = StateDir::signed_in("summary");
    let out = dir.status(&[]);
    assert!(out.starts_with("Monitoring stopped\n"), "{out}");
    assert!(out.contains("sam@example.org"), "{out}");
    assert!(out.contains("None yet"), "{out}");
    assert!(out.contains("waiting to upload:"), "{out}");
    assert!(out.contains("recent errors:"), "{out}");
    assert!(out.contains("`virtue status --full`"), "{out}");
    for hidden in [
        "Account", "Queues", "Capture", "Advanced", "boom", "dev-123",
    ] {
        assert!(!out.contains(hidden), "summary showed {hidden:?}:\n{out}");
    }
    assert!(
        !out.contains('\x1b'),
        "piped output has color codes:\n{out}"
    );
}

// CLIENT-007: `virtue status --full` shows every group but Advanced, and the
// What Virtue Monitors summary.
#[test]
fn status_full_leaves_out_advanced() {
    let dir = StateDir::signed_in("full");
    let out = dir.status(&["--full"]);
    for shown in [
        "Account",
        "Queues",
        "Capture",
        "Recent errors",
        "boom",
        "What Virtue monitors",
    ] {
        assert!(out.contains(shown), "--full is missing {shown:?}:\n{out}");
    }
    assert!(
        !out.contains("\nAdvanced\n"),
        "--full showed Advanced:\n{out}"
    );
    assert!(
        !out.contains("dev-123"),
        "--full showed the device id:\n{out}"
    );
}

// CLIENT-007: `virtue status --advanced` shows every group.
#[test]
fn status_advanced_shows_everything() {
    let dir = StateDir::signed_in("advanced");
    let out = dir.status(&["--advanced"]);
    for shown in [
        "Account",
        "Queues",
        "Capture",
        "Recent errors",
        "\nAdvanced\n",
        "dev-123",
    ] {
        assert!(
            out.contains(shown),
            "--advanced is missing {shown:?}:\n{out}"
        );
    }
}

// CLIENT-020: --json prints every field whatever the other flags.
#[test]
fn status_json_ignores_detail_flags() {
    let dir = StateDir::signed_in("json");
    let value: serde_json::Value = serde_json::from_str(&dir.status(&["--json"])).unwrap();
    assert_eq!(value["device_id"], "dev-123");
    assert!(value.get("state_dir").is_some());
}
