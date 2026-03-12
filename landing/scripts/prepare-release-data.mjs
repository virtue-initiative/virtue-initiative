import { access, mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { syncReleaseData } from "./fetch-release-data.mjs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, "..");
const outputPath = path.join(projectRoot, "src", "data", "releases.json");

async function fileExists(filePath) {
  try {
    await access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function writeFallbackReleaseData() {
  await mkdir(path.dirname(outputPath), { recursive: true });
  await writeFile(
    outputPath,
    `${JSON.stringify(
      {
        generatedAt: new Date().toISOString(),
        stableRelease: null,
        prereleaseRelease: null,
      },
      null,
      2,
    )}\n`,
  );
  console.log(`Wrote fallback release data to ${outputPath}.`);
}

async function main() {
  if (process.env.VIRTUE_REQUIRE_RELEASE_SYNC === "1") {
    await syncReleaseData();
    return;
  }

  if (await fileExists(outputPath)) {
    console.log(`Using existing release data at ${outputPath}.`);
    return;
  }

  await writeFallbackReleaseData();
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
