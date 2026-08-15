// The whole codebase shares one version, tracked in client/version.properties. This is
// that version's `/vX`/`/vX.Y` URL-prefix form (api/SPEC.md section 1.4,
// hash-server/SPEC.md section 1.3) — kept in sync by client/scripts/update-version.sh,
// which is the only thing that should ever edit this line.
export const CURRENT_API_VERSION = 'v0.1';
