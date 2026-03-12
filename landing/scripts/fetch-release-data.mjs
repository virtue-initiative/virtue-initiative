import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, "..");
const outputPath = path.join(projectRoot, "src", "data", "releases.json");

const GITHUB_RELEASES_URL =
  "https://api.github.com/repos/virtue-initiative/virtue-initiative/releases?per_page=20";
const TIMEOUT_MS = 20 * 60 * 1000;
const AUTHENTICATED_INTERVAL_MS = 30 * 1000;
const UNAUTHENTICATED_INTERVAL_MS = 60 * 1000;
const EXPECTED_SHA = process.env.GITHUB_SHA?.toLowerCase() ?? null;
const EXPECTED_SHORT_SHA = EXPECTED_SHA?.slice(0, 7) ?? null;
const HAS_GITHUB_TOKEN = Boolean(process.env.GITHUB_TOKEN);
const INTERVAL_MS = HAS_GITHUB_TOKEN
  ? AUTHENTICATED_INTERVAL_MS
  : UNAUTHENTICATED_INTERVAL_MS;

const expectedPlatforms = [
  {
    label: "Linux",
    matches: (assetName) =>
      /\.deb$/i.test(assetName) && /^virtue_/i.test(assetName),
  },
  {
    label: "macOS",
    matches: (assetName) =>
      /\.dmg$/i.test(assetName) && /^Virtue-/i.test(assetName),
  },
  {
    label: "Windows",
    matches: (assetName) =>
      /\.exe$/i.test(assetName) && /windows-installer/i.test(assetName),
  },
  {
    label: "Android",
    matches: (assetName) =>
      /\.apk$/i.test(assetName) && /android/i.test(assetName),
  },
  {
    label: "iOS",
    matches: (assetName) =>
      /\.zip$/i.test(assetName) && /^VirtueIOS/i.test(assetName),
  },
];

function byNewestPublished(left, right) {
  return (
    new Date(right.published_at ?? 0).getTime() -
    new Date(left.published_at ?? 0).getTime()
  );
}

function pickReleaseFields(release) {
  if (!release) return null;

  return {
    name: release.name,
    tag_name: release.tag_name,
    html_url: release.html_url,
    published_at: release.published_at,
    target_commitish: release.target_commitish,
    assets: release.assets.map((asset) => ({
      name: asset.name,
      browser_download_url: asset.browser_download_url,
    })),
  };
}

function assetMatchesCurrentBuild(assetName) {
  if (!EXPECTED_SHORT_SHA) return true;

  return assetName.toLowerCase().includes(EXPECTED_SHORT_SHA);
}

function releaseTargetsCurrentCommit(release) {
  if (!release || !EXPECTED_SHA) return true;

  return String(release.target_commitish ?? "").toLowerCase() === EXPECTED_SHA;
}

function missingPlatforms(release) {
  if (!release) return expectedPlatforms.map((platform) => platform.label);

  return expectedPlatforms
    .filter(
      (platform) =>
        !release.assets.some(
          (asset) =>
            platform.matches(asset.name) && assetMatchesCurrentBuild(asset.name),
        ),
    )
    .map((platform) => platform.label);
}

async function fetchReleases() {
  const headers = {
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "virtue-initiative-landing-release-poller",
  };

  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  }

  const response = await fetch(GITHUB_RELEASES_URL, { headers });

  if (!response.ok) {
    const body = await response.text();
    throw new Error(`GitHub API failed: ${response.status} ${body}`.trim());
  }

  return response.json();
}

async function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function main() {
  const deadline = Date.now() + TIMEOUT_MS;
  let latestPrerelease = null;
  let latestStableRelease = null;

  console.log(
    `Polling GitHub releases every ${INTERVAL_MS / 1000}s (${
      HAS_GITHUB_TOKEN ? "authenticated" : "unauthenticated"
    })${EXPECTED_SHORT_SHA ? ` for commit ${EXPECTED_SHORT_SHA}` : ""}.`,
  );

  while (Date.now() <= deadline) {
    const releases = (await fetchReleases()).filter(
      (release) => !release.draft,
    );

    latestStableRelease =
      releases
        .filter((release) => !release.prerelease)
        .sort(byNewestPublished)[0] ?? null;
    latestPrerelease =
      releases
        .filter((release) => release.prerelease)
        .sort(byNewestPublished)[0] ?? null;

    const matchesCommit = releaseTargetsCurrentCommit(latestPrerelease);
    const missing = matchesCommit ? missingPlatforms(latestPrerelease) : [];

    if (matchesCommit && missing.length === 0) {
      await mkdir(path.dirname(outputPath), { recursive: true });
      await writeFile(
        outputPath,
        `${JSON.stringify(
          {
            generatedAt: new Date().toISOString(),
            stableRelease: pickReleaseFields(latestStableRelease),
            prereleaseRelease: pickReleaseFields(latestPrerelease),
          },
          null,
          2,
        )}\n`,
      );

      console.log(
        `Wrote release data to ${outputPath} using prerelease ${latestPrerelease.tag_name}.`,
      );
      return;
    }

    const remainingSeconds = Math.max(
      0,
      Math.ceil((deadline - Date.now()) / 1000),
    );

    if (!matchesCommit) {
      console.log(
        `Latest prerelease ${
          latestPrerelease?.tag_name ?? "none"
        } does not target the current commit yet. Expected ${
          EXPECTED_SHORT_SHA ?? "current sha"
        }, got ${latestPrerelease?.target_commitish ?? "none"}. Polling again in ${
          INTERVAL_MS / 1000
        }s (${remainingSeconds}s remaining).`,
      );
    } else {
      console.log(
        `Latest prerelease for ${
          EXPECTED_SHORT_SHA ?? latestPrerelease?.target_commitish ?? "current build"
        } is not complete yet. Missing: ${missing.join(", ")}. Polling again in ${
          INTERVAL_MS / 1000
        }s (${remainingSeconds}s remaining).`,
      );
    }

    await sleep(INTERVAL_MS);
  }

  throw new Error(
    `Timed out after ${TIMEOUT_MS / 60000} minutes waiting for a complete prerelease. Latest prerelease: ${
      latestPrerelease?.tag_name ?? "none"
    }. Target commit: ${latestPrerelease?.target_commitish ?? "none"}. Expected commit: ${
      EXPECTED_SHA ?? "not specified"
    }. Missing: ${missingPlatforms(latestPrerelease).join(", ") || "unknown"}.`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
