use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use chrono::Utc;

fn main() {
    load_dotenv();

    println!("cargo:rerun-if-env-changed=VIRTUE_BUILD_LABEL");
    println!("cargo:rerun-if-env-changed=VIRTUE_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=VIRTUE_GIT_SHORT_HASH");
    println!("cargo:rerun-if-env-changed=VIRTUE_GIT_REF_NAME");
    println!("cargo:rerun-if-env-changed=VIRTUE_RELEASE_CHANNEL");
    println!("cargo:rerun-if-env-changed=VIRTUE_DEFAULT_API_URL");
    println!("cargo:rerun-if-env-changed=VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS");
    println!("cargo:rerun-if-env-changed=VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-changed=../version.properties");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../.env");

    assert_model_resolved();

    let build_label = build_label();
    println!("cargo:rustc-env=VIRTUE_BUILD_LABEL={build_label}");

    let default_api_url = default_api_base_url();
    println!("cargo:rustc-env=VIRTUE_DEFAULT_API_URL={default_api_url}");

    let capture_interval_seconds = capture_interval_seconds();
    println!("cargo:rustc-env=VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS={capture_interval_seconds}");

    let batch_window_seconds = batch_window_seconds();
    println!("cargo:rustc-env=VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS={batch_window_seconds}");
}

/// Loads `client/.env` (sibling of `core/`) if present, setting each `KEY=VALUE` pair as a
/// process env var — but only if that key isn't already set, so real process/CI env vars
/// always take precedence over the file. Lets a developer set local compile-time defaults
/// (API URL, intervals) without exporting shell env vars.
fn load_dotenv() {
    let Ok(manifest_dir) = env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let dotenv_path = PathBuf::from(manifest_dir).join("../.env");
    let Ok(contents) = fs::read_to_string(&dotenv_path) else {
        return;
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if key.is_empty() {
            continue;
        }
        if env::var(key).is_err() {
            unsafe {
                env::set_var(key, value);
            }
        }
    }
}

/// The NSFW model is tracked by Git LFS and embedded into the binary via `include_bytes!`
/// (`src/module/screenshot.rs`) — as an NNEF tar pre-converted offline from the source ONNX
/// model (see `examples/onnx_to_nnef.rs`; both files are LFS-tracked, but only the NNEF one is
/// actually compiled in). If LFS objects aren't materialized at build time, the file on disk is
/// a ~130-byte text *pointer*, which gets baked into the binary instead of the model. The
/// classifier then fails to load and every screenshot risk is silently 0. Catch that here at
/// build time — loudly — instead of shipping a broken detector.
fn assert_model_resolved() {
    const LFS_POINTER_MAGIC: &[u8] = b"version https://git-lfs.github.com/spec/v1";
    // The real NNEF model is ~17 MB; any LFS pointer is well under 1 KiB. Anything below this
    // is certainly not a usable model.
    const MIN_MODEL_BYTES: u64 = 4096;

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let model_path = manifest_dir.join("models/nsfw_small_v1.nnef.tar");
    println!("cargo:rerun-if-changed=models/nsfw_small_v1.nnef.tar");

    let metadata = fs::metadata(&model_path).unwrap_or_else(|err| {
        panic!(
            "NSFW model {} is missing ({err}). Run `git lfs install && git lfs pull` before building.",
            model_path.display()
        )
    });

    let is_pointer = fs::read(&model_path)
        .map(|bytes| bytes.starts_with(LFS_POINTER_MAGIC))
        .unwrap_or(false);

    if is_pointer || metadata.len() < MIN_MODEL_BYTES {
        panic!(
            "NSFW model {} is an unresolved Git LFS pointer ({} bytes), not the real NNEF. \
             Run `git lfs install && git lfs pull` (and ensure CI checks out with `lfs: true`) \
             before building, or the screenshot risk classifier will silently report 0.",
            model_path.display(),
            metadata.len()
        );
    }
}

fn build_label() -> String {
    env::var("VIRTUE_BUILD_LABEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("{}-{}-{}", release_tag(), build_date(), git_short_hash()))
}

fn default_api_base_url() -> String {
    env::var("VIRTUE_DEFAULT_API_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            if release_channel() == "stable" {
                "https://api.virtueinitiative.org".to_string()
            } else {
                "https://staging.app.virtueinitiative.org/api".to_string()
            }
        })
}

/// Enforced at build time (panics on violation) rather than clamped silently at runtime, so a
/// misconfigured interval fails fast in CI/build instead of shipping a silently-adjusted value.
const MIN_CAPTURE_INTERVAL_SECONDS: u64 = 15;
const MIN_BATCH_INTERVAL_SECONDS: u64 = 1;

fn capture_interval_seconds() -> u64 {
    build_time_u64_env(
        "VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS",
        300,
        MIN_CAPTURE_INTERVAL_SECONDS,
    )
}

fn batch_window_seconds() -> u64 {
    build_time_u64_env(
        "VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS",
        3600,
        MIN_BATCH_INTERVAL_SECONDS,
    )
}

fn build_time_u64_env(key: &str, default: u64, floor: u64) -> u64 {
    let value = match env::var(key).ok().filter(|v| !v.trim().is_empty()) {
        Some(raw) => raw
            .trim()
            .parse::<u64>()
            .unwrap_or_else(|err| panic!("{key}={raw:?} is not a valid u64: {err}")),
        None => default,
    };

    if value < floor {
        panic!("{key}={value} is below the minimum allowed value of {floor}");
    }

    value
}

fn release_tag() -> String {
    let base_version = base_version();
    if release_channel() == "stable" {
        base_version
    } else {
        format!("{base_version}-dev")
    }
}

fn release_channel() -> String {
    match env::var("VIRTUE_RELEASE_CHANNEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        Some(value) if value == "stable" || value == "dev" => value,
        Some(value) => panic!("Unsupported VIRTUE_RELEASE_CHANNEL: {value}"),
        None => {
            if git_ref_name() == "main" {
                "stable".to_string()
            } else {
                "dev".to_string()
            }
        }
    }
}

fn git_ref_name() -> String {
    env::var("VIRTUE_GIT_REF_NAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("GITHUB_REF_NAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| git_output(&["rev-parse", "--abbrev-ref", "HEAD"]))
        .filter(|value| !value.trim().is_empty() && value != "HEAD")
        .unwrap_or_else(|| "detached".to_string())
}

fn git_short_hash() -> String {
    env::var("VIRTUE_GIT_SHORT_HASH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("GITHUB_SHA").ok().and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.chars().take(7).collect::<String>())
                }
            })
        })
        .or_else(|| git_output(&["rev-parse", "--short", "HEAD"]))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn build_date() -> String {
    env::var("VIRTUE_BUILD_DATE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string())
}

fn base_version() -> String {
    version_property("VERSION").unwrap_or_else(|| {
        env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION should always be set")
    })
}

fn version_property(key: &str) -> Option<String> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let version_file = manifest_dir.join("../version.properties");
    let contents = fs::read_to_string(version_file).ok()?;

    contents.lines().find_map(|line| {
        let (raw_key, raw_value) = line.split_once('=')?;
        if raw_key.trim() == key {
            Some(raw_value.trim().to_string())
        } else {
            None
        }
    })
}

fn git_output(args: &[&str]) -> Option<String> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let repo_root = manifest_dir.join("../..");
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
