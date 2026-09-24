#!/usr/bin/env bun
// Seeds the local dev database (D1) and bucket (R2) with sample accounts,
// devices, partnerships and a week of encrypted activity, so the web app has
// something realistic to show without a real device.
//
//   bun scripts/seed/index.ts               # accounts + devices + sample batches
//   bun scripts/seed/index.ts --users-only  # just the accounts (fast)
//
// Idempotent: accounts, devices and partnerships are upserted by fixed ids, and
// every batch belonging to a seeded device is deleted and regenerated relative
// to the current time. Re-run it (`just seed`) whenever the sample week gets
// stale, then use "Clear cache" in the web app so it drops the old batches.
//
// Writes go straight to the same local state `wrangler dev --env staging
// --local` reads (api/.wrangler/state/v3), through wrangler's platform proxy,
// so no servers need to be running.
import type { D1Database, D1PreparedStatement, R2Bucket } from '@cloudflare/workers-types';
import { getPlatformProxy } from 'wrangler';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { deriveAccount, PASSWORD, type AccountMaterial } from './accounts';
import { buildBatch } from './batch';
import { DEV, DEVICES, generateBatches, PARTNERSHIPS, USERS } from './scenario';
import { hexToBytes, hexToUuid } from './util';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const usersOnly = process.argv.includes('--users-only');

type Env = { DB: D1Database; BUCKET: R2Bucket; R2_URL: string };

const accounts = new Map<string, AccountMaterial>();
for (const user of USERS) {
  accounts.set(user.idHex, await deriveAccount(user));
}

const proxy = await getPlatformProxy<Env>({
  configPath: join(ROOT, 'api', 'wrangler.json'),
  environment: 'staging',
  persist: { path: join(ROOT, 'api', '.wrangler', 'state', 'v3') },
});
const { DB, BUCKET, R2_URL } = proxy.env;
const blob = (hex: string) => hexToBytes(hex);
const now = Date.now();

try {
  const statements: D1PreparedStatement[] = [];

  for (const user of USERS) {
    const account = accounts.get(user.idHex)!;
    statements.push(
      DB.prepare(
        `INSERT INTO users (id, email, name, password_hash, password_salt, password_params_version,
                            email_verified, pub_key, encrypted_priv_key)
         VALUES (?, ?, ?, ?, ?, 'argon2id-v1', 1, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           email = excluded.email, name = excluded.name, password_hash = excluded.password_hash,
           password_salt = excluded.password_salt, password_params_version = 'argon2id-v1',
           email_verified = 1, pub_key = excluded.pub_key,
           encrypted_priv_key = excluded.encrypted_priv_key`,
      ).bind(
        blob(user.idHex),
        user.email,
        user.name,
        account.passwordHashHex,
        blob(account.saltHex),
        account.pubKey,
        blob(account.encryptedPrivKeyHex),
      ),
    );
  }

  // The dev account is an admin so /admin works out of the box locally (API-051).
  statements.push(
    DB.prepare('INSERT OR IGNORE INTO admins (user_id, created_at) VALUES (?, ?)').bind(
      blob(DEV.idHex),
      now,
    ),
  );

  if (!usersOnly) {
    for (const device of DEVICES) {
      statements.push(
        DB.prepare(
          `INSERT INTO devices (id, owner, name, platform, enabled, created_at, deleted_at)
           VALUES (?, ?, ?, ?, 1, ?, NULL)
           ON CONFLICT(id) DO UPDATE SET
             owner = excluded.owner, name = excluded.name, platform = excluded.platform,
             enabled = 1, deleted_at = NULL`,
        ).bind(blob(device.idHex), blob(device.owner.idHex), device.name, device.platform, now),
      );
    }

    for (const { idHex, watching, watcher } of PARTNERSHIPS) {
      // Clear any hand-made invite between the same two people first, since
      // (watching_user_id, watcher_email) is unique.
      statements.push(
        DB.prepare('DELETE FROM partners WHERE watching_user_id = ? AND watcher_email = ?').bind(
          blob(watching.idHex),
          watcher.email,
        ),
        DB.prepare(
          `INSERT INTO partners (id, watching_user_id, watcher_user_id, watcher_email, status, created_at, updated_at)
           VALUES (?, ?, ?, ?, 'accepted', ?, ?)`,
        ).bind(blob(idHex), blob(watching.idHex), blob(watcher.idHex), watcher.email, now, now),
      );
    }
  }

  await DB.batch(statements);

  if (!usersOnly) {
    // Drop the previous run's batches (D1 rows and R2 blobs) for seeded devices.
    const deviceIds = DEVICES.map((d) => blob(d.idHex));
    const placeholders = deviceIds.map(() => '?').join(', ');
    const old = await DB.prepare(`SELECT url FROM batches WHERE device_id IN (${placeholders})`)
      .bind(...deviceIds)
      .all<{ url: string }>();
    const oldKeys = old.results
      .map((row) => row.url.split('/r2/')[1])
      .filter((key): key is string => !!key);
    for (let i = 0; i < oldKeys.length; i += 500) {
      await BUCKET.delete(oldKeys.slice(i, i + 500));
    }
    await DB.prepare(`DELETE FROM batches WHERE device_id IN (${placeholders})`)
      .bind(...deviceIds)
      .run();

    // Each owner's batches are readable by the owner and everyone watching them.
    const recipientsFor = (ownerHex: string) =>
      [
        ownerHex,
        ...PARTNERSHIPS.filter((p) => p.watching.idHex === ownerHex).map((p) => p.watcher.idHex),
      ].map((hex) => ({ uuid: hexToUuid(hex), pubKey: accounts.get(hex)!.pubKey }));

    const batches = generateBatches(now);
    const inserts: D1PreparedStatement[] = [];
    let eventCount = 0;
    for (const batch of batches) {
      const owner = batch.device.owner.idHex;
      const built = await buildBatch(batch.events, recipientsFor(owner));
      const id = crypto.randomUUID();
      const key = `user/${hexToUuid(owner)}/batches/${id}.enc`;
      await BUCKET.put(key, built.blob, {
        httpMetadata: { contentType: 'application/octet-stream' },
      });
      inserts.push(
        DB.prepare(
          `INSERT INTO batches (id, user_id, device_id, url, start_time, end_time, end_hash, access_keys,
                                version, high_risk_count, medium_risk_count, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'v0.1', ?, ?, ?)`,
        ).bind(
          blob(id.replace(/-/g, '')),
          blob(owner),
          blob(batch.device.idHex),
          `${R2_URL}/${key}`,
          batch.startTime,
          batch.endTime,
          built.endHashHex,
          JSON.stringify(built.accessKeys),
          built.highRiskCount,
          built.mediumRiskCount,
          // Uploaded shortly after the last event, like a real client.
          Math.min(now, batch.endTime + 60_000),
        ),
      );
      eventCount += batch.events.length;
    }
    for (let i = 0; i < inserts.length; i += 50) {
      await DB.batch(inserts.slice(i, i + 50));
    }
    console.log(
      `Seeded ${DEVICES.length} devices, ${batches.length} batches, ${eventCount} events ` +
        `(replaced ${old.results.length} old batches).`,
    );
  }

  console.log('Accounts (all use password "' + PASSWORD + '"):');
  for (const user of USERS) console.log(`  ${user.email}${user === DEV ? ' (admin)' : ''}`);
  if (!usersOnly) {
    for (const { watching, watcher } of PARTNERSHIPS) {
      console.log(`  ${watcher.email} watches ${watching.email}`);
    }
    console.log('If the web app was already open, use "Clear cache" so it drops the old batches.');
  }
} finally {
  await proxy.dispose();
}
