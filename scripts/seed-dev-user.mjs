#!/usr/bin/env bun
// Inserts a deterministic dev test account into the local D1 database.
// Idempotent: uses INSERT OR IGNORE, so re-running setup is safe.
//
// Credentials: dev@dev.com / devpassword
import { argon2id } from 'hash-wasm';
import { execSync } from 'child_process';
import { writeFileSync, unlinkSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');

const EMAIL = 'dev@dev.com';
const PASSWORD = 'devpassword';

// Fixed UUID and salt so the seed is deterministic across runs.
const USER_ID_HEX = '0123456789abcdef0123456789abcdef';
const PASSWORD_SALT = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);

// Matches CURRENT_HASH_PARAMS in api/src/lib/password.ts
const PARAMS = { time_cost: 5, memory_cost_kib: 131_072, parallelism: 1 };

function toHex(bytes) {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

async function hkdfSha256(label, ikm) {
  const key = await crypto.subtle.importKey('raw', ikm, 'HKDF', false, ['deriveBits']);
  const bits = await crypto.subtle.deriveBits(
    {
      name: 'HKDF',
      hash: 'SHA-256',
      salt: new Uint8Array(),
      info: new TextEncoder().encode(label),
    },
    key,
    256,
  );
  return new Uint8Array(bits);
}

const argonOutput = await argon2id({
  password: PASSWORD,
  salt: PASSWORD_SALT,
  iterations: PARAMS.time_cost,
  memorySize: PARAMS.memory_cost_kib,
  hashLength: 32,
  parallelism: PARAMS.parallelism,
  outputType: 'binary',
});

const passwordAuthBytes = await hkdfSha256('auth', argonOutput);
const hashBuffer = await crypto.subtle.digest('SHA-256', passwordAuthBytes);
const passwordHash = toHex(new Uint8Array(hashBuffer));
const saltHex = toHex(PASSWORD_SALT);

const sql = `INSERT OR IGNORE INTO users (id, email, password_hash, password_salt, password_params_version, email_verified)
VALUES (X'${USER_ID_HEX}', '${EMAIL}', '${passwordHash}', X'${saltHex}', 'argon2id-v1', 1);`;

const tmpFile = join(ROOT, '.seed-dev-user-tmp.sql');
writeFileSync(tmpFile, sql);

try {
  execSync(
    `cd "${ROOT}/api" && bunx wrangler d1 execute DB --env staging --local --file "${tmpFile}"`,
    { stdio: 'pipe' },
  );
  console.log(`Dev account ready: ${EMAIL} / ${PASSWORD}`);
} finally {
  unlinkSync(tmpFile);
}
