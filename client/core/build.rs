use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let client_root = manifest_dir
        .parent()
        .expect("core crate should live under client/")
        .to_path_buf();
    let repo_root = client_root
        .parent()
        .expect("client dir should live under repo root")
        .to_path_buf();
    let version_file = client_root.join("version.properties");

    // Always rerun when .env.dev is added, changed, or removed.
    println!("cargo:rerun-if-changed=.env.dev");
    println!("cargo:rerun-if-changed={}", version_file.display());
    println!("cargo:rerun-if-env-changed=VIRTUE_GIT_SHORT_HASH");
    println!("cargo:rerun-if-env-changed=VIRTUE_BUILD_LABEL");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    emit_git_rerun_hints(&repo_root);

    dotenv_build::output(dotenv_build::Config {
        filename: std::path::Path::new(".env.dev"),
        recursive_search: false,
        fail_if_missing_dotenv: false,
    })
    .unwrap();

    let version_props = load_properties(&version_file);
    let base_version = version_props
        .get("VERSION")
        .cloned()
        .expect("VERSION missing from client/version.properties");
    let package_version = std::env::var("CARGO_PKG_VERSION").expect("package version");
    if base_version != package_version {
        panic!(
            "client/version.properties VERSION ({base_version}) does not match {package_version} from CARGO_PKG_VERSION"
        );
    }

    let git_short_hash = resolve_git_short_hash(&repo_root);
    let build_label = std::env::var("VIRTUE_BUILD_LABEL")
        .unwrap_or_else(|_| format!("{base_version}-{git_short_hash}"));

    println!("cargo:rustc-env=VIRTUE_BASE_VERSION={base_version}");
    println!("cargo:rustc-env=VIRTUE_GIT_SHORT_HASH={git_short_hash}");
    println!("cargo:rustc-env=VIRTUE_BUILD_LABEL={build_label}");
}

fn load_properties(path: &Path) -> HashMap<String, String> {
    let contents = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let mut values = HashMap::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        values.insert(key.trim().to_string(), value.trim().to_string());
    }

    values
}

fn resolve_git_short_hash(repo_root: &Path) -> String {
    if let Ok(hash) = std::env::var("VIRTUE_GIT_SHORT_HASH") {
        return hash;
    }

    if let Ok(hash) = std::env::var("GITHUB_SHA") {
        return hash.chars().take(7).collect();
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .unwrap_or_else(|err| panic!("failed to run git rev-parse: {err}"));

    if !output.status.success() {
        panic!("git rev-parse failed with status {}", output.status);
    }

    String::from_utf8(output.stdout)
        .expect("git hash should be utf-8")
        .trim()
        .to_string()
}

fn emit_git_rerun_hints(repo_root: &Path) {
    let git_dir = repo_root.join(".git");
    let head_path = git_dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());

    if let Ok(head_contents) = std::fs::read_to_string(&head_path)
        && let Some(reference) = head_contents.trim().strip_prefix("ref: ")
    {
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join(reference).display()
        );
    }
}
