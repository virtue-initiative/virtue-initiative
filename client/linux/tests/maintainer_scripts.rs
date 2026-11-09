//! Runs the real `.deb` maintainer scripts against stub `su`, `runuser`,
//! `systemctl` and `loginctl` commands on `PATH`, so they can be checked
//! without root or a real systemd user manager.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    /// `systemctl_exit` is what every stubbed `systemctl` call exits with.
    fn new(name: &str, systemctl_exit: i32) -> Self {
        let dir =
            std::env::temp_dir().join(format!("virtue-maintainer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        fs::create_dir_all(&bin).unwrap();

        let uid = run_id(&["-u"]);
        let user = run_id(&["-un"]);
        fs::create_dir_all(dir.join("run").join(&uid)).unwrap();

        let log = dir.join("calls.log");
        let log = log.display();
        // A login shell sources ~/.profile, so a broken one makes `su -` fail
        // before it runs the command.
        stub(&bin, "su", &format!("echo \"su $*\" >> {log}\nexit 1"));
        stub(
            &bin,
            "runuser",
            &format!(
                "echo \"runuser $*\" >> {log}\n\
                 for arg in \"$@\"; do\n\
                   case \"$arg\" in -|-l|--login) exit 1 ;; --) break ;; esac\n\
                 done\n\
                 while [ \"$1\" != \"--\" ]; do shift; done\n\
                 shift\n\
                 exec \"$@\""
            ),
        );
        stub(
            &bin,
            "systemctl",
            &format!("echo \"systemctl $*\" >> {log}\nexit {systemctl_exit}"),
        );
        stub(&bin, "loginctl", &format!("echo \"{uid} {user} seat0\""));

        Sandbox { dir }
    }

    fn run(&self, script: &str, arg: &str) -> Output {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("packaging/debian")
            .join(script);
        Command::new("sh")
            .arg(path)
            .arg(arg)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.dir.join("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("VIRTUE_RUN_USER_DIR", self.dir.join("run"))
            .env_remove("SUDO_USER")
            .output()
            .unwrap()
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.dir.join("calls.log")).unwrap_or_default()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn run_id(args: &[&str]) -> String {
    let output = Command::new("id").args(args).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn stub(bin: &Path, name: &str, body: &str) {
    let path = bin.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "script failed: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// CLIENT-003: install starts the service for each logged-in user without a
// login shell, so a broken ~/.profile cannot fail it.
#[test]
fn postinst_starts_service_without_login_shell() {
    let sandbox = Sandbox::new("postinst", 0);
    let output = sandbox.run("postinst", "configure");
    assert_success(&output);

    let calls = sandbox.calls();
    assert!(!calls.contains("su "), "postinst used su:\n{calls}");
    assert!(
        calls.contains("systemctl --user enable virtue.service"),
        "service was not enabled for the user:\n{calls}"
    );
    assert!(
        calls.contains("systemctl --user restart virtue.service")
            || calls.contains("systemctl --user start virtue.service"),
        "service was not started for the user:\n{calls}"
    );
}

// CLIENT-003: a failure for one user does not fail the install, and the
// install ends by telling the user to run `virtue login`.
#[test]
fn postinst_survives_user_failures_and_points_to_login() {
    let sandbox = Sandbox::new("postinst-fail", 1);
    let output = sandbox.run("postinst", "configure");
    assert_success(&output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("virtue login"),
        "postinst did not mention `virtue login`"
    );
}

// CLIENT-014: removal stops and disables the service for each logged-in user
// without a login shell.
#[test]
fn prerm_stops_service_without_login_shell() {
    let sandbox = Sandbox::new("prerm", 0);
    let output = sandbox.run("prerm", "remove");
    assert_success(&output);

    let calls = sandbox.calls();
    assert!(!calls.contains("su "), "prerm used su:\n{calls}");
    assert!(
        calls.contains("systemctl --user disable --now virtue.service"),
        "service was not stopped for the user:\n{calls}"
    );
    assert!(
        calls.contains("systemctl --global disable virtue.service"),
        "global enablement was not removed:\n{calls}"
    );
}
