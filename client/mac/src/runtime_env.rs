use virtue_client_core::{apply_dev_env, apply_env_defaults_from_map, load_env_file};

use crate::config::ClientPaths;

/// Applies debug `.env.dev` values and optional runtime service overrides.
/// Process environment variables always win over file values.
pub fn apply_runtime_env(paths: &ClientPaths) {
    apply_dev_env();
    let overrides = load_env_file(&paths.service_env_file);
    if !overrides.is_empty() {
        apply_env_defaults_from_map(&overrides);
    }
}
