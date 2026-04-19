use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use chrono::Utc;

fn main() {
    println!("cargo:rerun-if-env-changed=VIRTUE_BUILD_LABEL");
    println!("cargo:rerun-if-env-changed=VIRTUE_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=VIRTUE_GIT_SHORT_HASH");
    println!("cargo:rerun-if-env-changed=VIRTUE_GIT_REF_NAME");
    println!("cargo:rerun-if-env-changed=VIRTUE_RELEASE_CHANNEL");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-changed=../version.properties");
    println!("cargo:rerun-if-changed=../../.git/HEAD");

    let build_label = build_label();
    println!("cargo:rustc-env=VIRTUE_BUILD_LABEL={build_label}");
}

fn build_label() -> String {
    env::var("VIRTUE_BUILD_LABEL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("{}-{}-{}", release_tag(), build_date(), git_short_hash()))
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
