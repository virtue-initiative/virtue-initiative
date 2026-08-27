import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Assembles the Sparkle appcast served at https://virtueinitiative.org/appcast.xml.
 *
 * Why the feed lives here rather than on GitHub Releases: Sparkle needs ONE
 * URL that is baked into the app at build time and keeps returning newer
 * versions forever. No release-assets URL does that — `releases/latest/download/`
 * skips prereleases (so the dev channel would never update), and a per-tag URL
 * points at the release the app was built from, which by definition never
 * contains anything newer. The landing site is already rebuilt on every push to
 * main/staging and already polls the releases API, so it can serve a stable URL
 * whose contents track the newest release.
 *
 * Each macOS release publishes an `appcast-item-macos.xml` asset next to its
 * DMG — an EdDSA-signed <item> produced on the macOS runner, where the DMG and
 * the signing key are (see client/mac/scripts/make-appcast-item.sh). This
 * script just fetches those fragments and wraps them in a feed. It never signs
 * anything, so the release key never reaches this runner.
 *
 * One feed carries both channels: the stable release's item is untagged, and
 * the prerelease's item carries <sparkle:channel>dev</sparkle:channel>, which
 * only dev builds opt into. A stable app therefore ignores dev items even
 * though it can see them.
 *
 * Failure is deliberately non-fatal and fail-closed: a feed with no items means
 * "no update available", which is always safe. Breaking the whole site deploy
 * because a fragment 404'd would not be.
 */

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, '..');
const releaseDataPath = path.join(projectRoot, 'src', 'data', 'releases.json');
const outputPath = path.join(projectRoot, 'public', 'appcast.xml');

const APPCAST_ITEM_ASSET = 'appcast-item-macos.xml';

async function loadReleaseData() {
  try {
    return JSON.parse(await readFile(releaseDataPath, 'utf8'));
  } catch (error) {
    console.warn(`build-appcast: no release data at ${releaseDataPath} (${error.message}).`);
    return null;
  }
}

async function fetchItem(release, channel) {
  if (!release) {
    console.warn(`build-appcast: no ${channel} release in release data; skipping that channel.`);
    return null;
  }

  const asset = release.assets?.find((candidate) => candidate.name === APPCAST_ITEM_ASSET);
  if (!asset) {
    console.warn(
      `build-appcast: ${channel} release ${release.tag_name} has no ${APPCAST_ITEM_ASSET} asset; skipping that channel.`,
    );
    return null;
  }

  try {
    const response = await fetch(asset.browser_download_url, {
      headers: { Accept: 'application/xml' },
    });
    if (!response.ok) {
      console.warn(
        `build-appcast: fetching ${channel} item returned ${response.status}; skipping that channel.`,
      );
      return null;
    }

    const body = (await response.text()).trim();
    // The fragment is signed content produced by our own release job; a
    // response that isn't even shaped like an <item> means something is wrong
    // upstream and must not be pasted into the feed.
    if (!body.startsWith('<item>') || !body.endsWith('</item>')) {
      console.warn(`build-appcast: ${channel} item is not a well-formed <item>; skipping.`);
      return null;
    }
    console.log(`build-appcast: included ${channel} item from ${release.tag_name}.`);
    return body;
  } catch (error) {
    console.warn(`build-appcast: failed to fetch ${channel} item (${error.message}); skipping.`);
    return null;
  }
}

async function main() {
  const releaseData = await loadReleaseData();

  const items = (
    await Promise.all([
      fetchItem(releaseData?.stableRelease, 'stable'),
      fetchItem(releaseData?.prereleaseRelease, 'dev'),
    ])
  ).filter(Boolean);

  if (items.length === 0) {
    console.warn('build-appcast: no items resolved; publishing an empty feed (no update offered).');
  }

  const feed = `<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Virtue for macOS</title>
    <link>https://virtueinitiative.org/appcast.xml</link>
    <description>Automatic updates for the Virtue macOS client.</description>
    <language>en</language>
${items.map((item) => item.replace(/^/gm, '    ')).join('\n')}
  </channel>
</rss>
`;

  await mkdir(path.dirname(outputPath), { recursive: true });
  await writeFile(outputPath, feed);
  console.log(`build-appcast: wrote ${items.length} item(s) to ${outputPath}.`);
}

main().catch((error) => {
  // Still non-fatal: an unexpected crash here must not break the site deploy.
  console.error('build-appcast: unexpected failure, continuing without a feed update.', error);
});
