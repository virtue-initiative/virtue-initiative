#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const repoRoot = path.resolve(__dirname, "..");
const docPath = path.join(
  repoRoot,
  "landing/src/content/help/developer/security.mdx",
);
const sectionStart = "{/* CODE_REFERENCES:START */}";
const sectionEnd = "{/* CODE_REFERENCES:END */}";

const references = [
  {
    title: "Browser password derivation",
    description:
      "Argon2id output is split into `passwordAuth` and the AES wrapping key with HKDF-SHA256.",
    file: "web/src/crypto.ts",
    language: "ts",
    startMarker: "export async function derivePasswordMaterial(",
  },
  {
    title: "Browser private-key wrapping",
    description:
      "The web client encrypts the private key locally with AES-GCM before upload.",
    file: "web/src/crypto.ts",
    language: "ts",
    startMarker: "export async function encryptData(",
  },
  {
    title: "Browser X25519 key generation",
    description:
      "The web app generates the user keypair from the HPKE suite's X25519 KEM.",
    file: "web/src/crypto.ts",
    language: "ts",
    startMarker: "export async function generateUserKeyPair()",
  },
  {
    title: "Browser batch-key unwrapping",
    description:
      "Batch access envelopes are opened with HPKE and imported back into WebCrypto as AES keys.",
    file: "web/src/crypto.ts",
    language: "ts",
    startMarker: "export async function unwrapBatchKey(",
  },
  {
    title: "API password-auth hashing",
    description:
      "The server stores `SHA-256(password_auth)` instead of the raw client-derived bytes.",
    file: "api/src/lib/password.ts",
    language: "ts",
    startMarker: "export async function hashPasswordAuth(",
  },
  {
    title: "Native client password derivation",
    description:
      "The Rust client matches the browser flow with Argon2id and HKDF-SHA256(\"auth\").",
    file: "client/core/src/crypto.rs",
    language: "rs",
    startMarker: "pub fn derive_password_auth(",
  },
  {
    title: "Native client batch upload encryption",
    description:
      "Each batch gets a fresh AES key, then the client HPKE-wraps that key for every recipient.",
    file: "client/core/src/batch.rs",
    language: "rs",
    startMarker: "    pub fn build_upload(",
  },
];

function git(args) {
  return execFileSync("git", args, {
    cwd: repoRoot,
    encoding: "utf8",
  }).trim();
}

function getGitHubBaseUrl() {
  const remote = git(["remote", "get-url", "origin"]);

  if (remote.startsWith("git@github.com:")) {
    return `https://github.com/${remote
      .slice("git@github.com:".length)
      .replace(/\.git$/, "")}`;
  }

  if (remote.startsWith("https://github.com/")) {
    return remote.replace(/\.git$/, "");
  }

  throw new Error(`Unsupported origin remote for GitHub links: ${remote}`);
}

function getRequestedRef() {
  const refFlagIndex = process.argv.indexOf("--ref");
  if (refFlagIndex !== -1) {
    const ref = process.argv[refFlagIndex + 1];
    if (!ref) {
      throw new Error("Expected a git ref after --ref");
    }
    return ref;
  }

  return "main";
}

function lineNumberAt(content, index) {
  return content.slice(0, index).split("\n").length;
}

function findFunctionSnippet(filePath, startMarker) {
  const absolutePath = path.join(repoRoot, filePath);
  const content = fs.readFileSync(absolutePath, "utf8");
  const markerIndex = content.indexOf(startMarker);

  if (markerIndex === -1) {
    throw new Error(`Could not find marker "${startMarker}" in ${filePath}`);
  }

  const snippetStart = content.lastIndexOf("\n", markerIndex) + 1;
  const firstBrace = content.indexOf("{", markerIndex);

  if (firstBrace === -1) {
    throw new Error(`Could not find function body for ${startMarker} in ${filePath}`);
  }

  let depth = 0;
  let snippetEndBrace = -1;

  for (let index = firstBrace; index < content.length; index += 1) {
    const char = content[index];
    if (char === "{") {
      depth += 1;
    } else if (char === "}") {
      depth -= 1;
      if (depth === 0) {
        snippetEndBrace = index;
        break;
      }
    }
  }

  if (snippetEndBrace === -1) {
    throw new Error(`Could not determine the end of ${startMarker} in ${filePath}`);
  }

  const nextNewline = content.indexOf("\n", snippetEndBrace);
  const snippetEnd = nextNewline === -1 ? content.length : nextNewline;

  return {
    snippet: content.slice(snippetStart, snippetEnd).trimEnd(),
    startLine: lineNumberAt(content, snippetStart),
    endLine: lineNumberAt(content, snippetEndBrace),
  };
}

function buildGitHubUrl(baseUrl, ref, filePath, startLine, endLine) {
  return `${baseUrl}/blob/${encodeURIComponent(ref)}/${filePath}#L${startLine}-L${endLine}`;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function renderSection(baseUrl, ref) {
  const parts = [
    "## Code references",
    "",
    `The links and snippets below are generated from the current tree by \`node scripts/check-code-references.js\`. They currently point at Git ref \`${ref}\`.`,
    "",
  ];

  for (const reference of references) {
    const { snippet, startLine, endLine } = findFunctionSnippet(
      reference.file,
      reference.startMarker,
    );
    const githubUrl = buildGitHubUrl(
      baseUrl,
      ref,
      reference.file,
      startLine,
      endLine,
    );

    parts.push(`### ${reference.title}`);
    parts.push("");
    parts.push(reference.description);
    parts.push("");
    parts.push(
      `Source: [\`${reference.file}:L${startLine}-L${endLine}\`](${githubUrl})`,
    );
    parts.push("");
    parts.push(`\`\`\`${reference.language}`);
    parts.push(snippet);
    parts.push("```");
    parts.push("");
  }

  return parts.join("\n").trimEnd();
}

function updateDocument() {
  const baseUrl = getGitHubBaseUrl();
  const ref = getRequestedRef();
  const generatedSection = `${sectionStart}\n${renderSection(baseUrl, ref)}\n${sectionEnd}`;
  const existing = fs.readFileSync(docPath, "utf8");

  let next;
  if (existing.includes(sectionStart) && existing.includes(sectionEnd)) {
    const pattern = new RegExp(
      `${escapeRegExp(sectionStart)}[\\s\\S]*${escapeRegExp(sectionEnd)}`,
      "m",
    );
    next = existing.replace(pattern, generatedSection);
  } else {
    next = `${existing.trimEnd()}\n\n${generatedSection}\n`;
  }

  if (next !== existing) {
    fs.writeFileSync(docPath, next);
    console.log(`Updated ${path.relative(repoRoot, docPath)}`);
  } else {
    console.log(`Checked ${path.relative(repoRoot, docPath)}; no changes needed`);
  }
}

try {
  updateDocument();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
