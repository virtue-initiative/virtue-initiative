import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Writes the Android self-update feed served at
 * https://virtueinitiative.org/android-update.json (read by AppUpdater in
 * client/android).
 *
 * Only the stable release is offered: dev builds don't self-update. The feed
 * points at the stable release's APK and carries the SHA-256 GitHub reports
 * for it, which the app checks after downloading:
 *
 *   {"version": "0.1.5", "url": "https://github.com/.../virtue-android-....apk", "sha256": "<hex>"}
 *
 * Like build-appcast.mjs, failure is non-fatal and fail-closed: when anything
 * is missing the feed is `{}`, which the app reads as "no update available".
 */

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, '..');
const releaseDataPath = path.join(projectRoot, 'src', 'data', 'releases.json');
const outputPath = path.join(projectRoot, 'public', 'android-update.json');

const APK_PATTERN = /^virtue-android-.*\.apk$/i;
const VERSION_PATTERN = /^\d+(\.\d+)*$/;

async function buildFeed() {
  let releaseData;
  try {
    releaseData = JSON.parse(await readFile(releaseDataPath, 'utf8'));
  } catch (error) {
    console.warn(`build-android-update: no release data at ${releaseDataPath} (${error.message}).`);
    return {};
  }

  const release = releaseData?.stableRelease;
  if (!release) {
    console.warn('build-android-update: no stable release in release data.');
    return {};
  }
  if (!VERSION_PATTERN.test(release.tag_name ?? '')) {
    console.warn(`build-android-update: stable tag ${release.tag_name} is not a plain version.`);
    return {};
  }

  const asset = release.assets?.find((candidate) => APK_PATTERN.test(candidate.name));
  if (!asset) {
    console.warn(`build-android-update: stable release ${release.tag_name} has no APK asset.`);
    return {};
  }

  const sha256 = /^sha256:([0-9a-f]{64})$/i.exec(asset.digest ?? '')?.[1]?.toLowerCase();
  if (!sha256) {
    console.warn(`build-android-update: ${asset.name} has no SHA-256 digest.`);
    return {};
  }

  console.log(`build-android-update: offering ${asset.name} (${release.tag_name}).`);
  return { version: release.tag_name, url: asset.browser_download_url, sha256 };
}

async function main() {
  const feed = await buildFeed();
  await mkdir(path.dirname(outputPath), { recursive: true });
  await writeFile(outputPath, `${JSON.stringify(feed, null, 2)}\n`);
  console.log(`build-android-update: wrote ${outputPath}.`);
}

main().catch((error) => {
  // Non-fatal: an unexpected crash here must not break the site deploy.
  console.error(
    'build-android-update: unexpected failure, continuing without a feed update.',
    error,
  );
});
