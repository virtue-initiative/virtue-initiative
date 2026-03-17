/**
 * Removes all objects from the remote R2 bucket using the Cloudflare REST API.
 * Automatically reads credentials from wrangler's config if env vars are not set.
 * Optionally set CLOUDFLARE_ACCOUNT_ID and/or CLOUDFLARE_API_TOKEN to override.
 */

import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

function readWranglerToken() {
  const configPath = join(homedir(), '.config', '.wrangler', 'config', 'default.toml');
  const toml = readFileSync(configPath, 'utf8');
  const match = toml.match(/^oauth_token\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error(`Could not find oauth_token in ${configPath}`);
  return match[1];
}

let token = process.env.CLOUDFLARE_API_TOKEN;
if (!token) {
  try {
    token = readWranglerToken();
  } catch (e) {
    console.error('No CLOUDFLARE_API_TOKEN set and could not read wrangler token:', e.message);
    process.exit(1);
  }
}

const headers = { Authorization: `Bearer ${token}` };

let accountId = process.env.CLOUDFLARE_ACCOUNT_ID;
if (!accountId) {
  const res = await fetch('https://api.cloudflare.com/client/v4/accounts', { headers });
  const data = await res.json();
  if (!data.success || data.result.length === 0) {
    console.error('Could not fetch accounts:', data.errors);
    process.exit(1);
  }
  if (data.result.length > 1) {
    console.error(
      'Multiple accounts found. Set CLOUDFLARE_ACCOUNT_ID to one of:\n' +
        data.result.map((a) => `  ${a.id}  ${a.name}`).join('\n'),
    );
    process.exit(1);
  }
  accountId = data.result[0].id;
  console.log(`Using account: ${data.result[0].name} (${accountId})`);
}

const BUCKET_NAME = process.argv[2] === 'prod' ? 'virtueinitiative-images' : 'virtueinitiative-staging-images'
const BASE = `https://api.cloudflare.com/client/v4/accounts/${accountId}/r2/buckets/${BUCKET_NAME}`;
let totalDeleted = 0;
let cursor;

do {
  const url = new URL(`${BASE}/objects`);
  url.searchParams.set('limit', '1000');
  if (cursor) url.searchParams.set('cursor', cursor);

  const listRes = await fetch(url, { headers });
  const listData = await listRes.json();

  if (!listData.success) {
    console.error('List failed:', listData.errors);
    process.exit(1);
  }

  const objects = listData.result ?? [];
  for (const { key } of objects) {
    const delRes = await fetch(`${BASE}/objects/${encodeURIComponent(key)}`, {
      method: 'DELETE',
      headers,
    });
    const delData = await delRes.json();
    if (!delData.success) {
      console.error(`Failed to delete: ${key}`, delData.errors);
    }
  }

  totalDeleted += objects.length;
  if (objects.length > 0) console.log(`Deleted ${objects.length} objects...`);

  cursor = listData.result_info?.is_truncated ? listData.result_info.cursor : undefined;
} while (cursor);

console.log(`Done. Total objects deleted: ${totalDeleted}`);

// --- Drop all user tables from the remote D1 database ---

const DB_ID = 'ff636ee0-a8f9-44a1-8a16-f0a162cf1c73';
const DB_BASE = `https://api.cloudflare.com/client/v4/accounts/${accountId}/d1/database/${DB_ID}`;

async function d1Query(sql, { ignoreErrors = false } = {}) {
  const res = await fetch(`${DB_BASE}/query`, {
    method: 'POST',
    headers: { ...headers, 'Content-Type': 'application/json' },
    body: JSON.stringify({ sql }),
  });
  const data = await res.json();
  if (!data.success) {
    if (ignoreErrors) return null;
    throw new Error(JSON.stringify(data.errors));
  }
  return data.result;
}

const tablesResult = await d1Query(
  "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'd1_%' AND name != '_cf_KV'",
);
const tables = tablesResult[0].results.map((r) => r.name);

if (tables.length === 0) {
  console.log('No tables to drop.');
} else {
  // Drop in multiple passes to handle FK dependency ordering
  let remaining = [...tables];
  while (remaining.length > 0) {
    const before = remaining.length;
    const next = [];
    for (const table of remaining) {
      const result = await d1Query(`DROP TABLE IF EXISTS "${table}"`, { ignoreErrors: true });
      if (result === null) next.push(table); // failed, retry next pass
    }
    if (next.length === before) {
      throw new Error(`Could not drop tables (FK deadlock?): ${next.join(', ')}`);
    }
    remaining = next;
  }
  console.log(`Dropped tables: ${tables.join(', ')}`);
}

await d1Query('DELETE FROM d1_migrations');
console.log('Cleared d1_migrations.');
