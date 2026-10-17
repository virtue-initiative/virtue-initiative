type SqlValue = string | number | ArrayBuffer | Uint8Array | null;

function parseUserSettings(json: string | null): { email_frequency: string; timezone: string } {
  try {
    const parsed = JSON.parse(json ?? '{}') as Record<string, unknown>;
    return {
      email_frequency:
        typeof parsed.email_frequency === 'string' ? parsed.email_frequency : 'daily',
      timezone: typeof parsed.timezone === 'string' ? parsed.timezone : 'UTC',
    };
  } catch {
    return { email_frequency: 'daily', timezone: 'UTC' };
  }
}

export function uuidToBytes(uuid: string): ArrayBuffer {
  const normalized = normalizeUuidString(uuid);
  const hex = normalized.replace(/-/g, '');

  if (!hex) {
    throw new Error(`Invalid UUID: ${uuid}`);
  }

  const bytes = new Uint8Array(16);

  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }

  return bytes.buffer;
}

function normalizeUuidString(uuid: string): string {
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(uuid)) {
    return uuid.toLowerCase();
  }

  if (/^[0-9a-f]{32}$/i.test(uuid)) {
    const hex = uuid.toLowerCase();
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }

  throw new Error(`Invalid UUID: ${uuid}`);
}

export function bytesToUuid(value: unknown): string {
  if (typeof value === 'string') {
    return normalizeUuidString(value);
  }

  const bytes =
    value instanceof ArrayBuffer
      ? new Uint8Array(value)
      : ArrayBuffer.isView(value)
        ? new Uint8Array(value.buffer, value.byteOffset, value.byteLength)
        : Array.isArray(value)
          ? Uint8Array.from(value)
          : null;

  if (!bytes) {
    throw new Error('Expected a UUID BLOB from the database');
  }

  if (bytes.byteLength !== 16) {
    const text = new TextDecoder().decode(bytes);
    return normalizeUuidString(text);
  }

  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');

  return normalizeUuidString(hex);
}

function decodeUuidFields<T extends Record<string, unknown>>(row: T, fields: string[]) {
  const mutableRow: Record<string, unknown> = row;

  for (const field of fields) {
    const value = mutableRow[field];

    if (value !== null && value !== undefined) {
      mutableRow[field] = bytesToUuid(value);
    }
  }

  return row;
}

async function firstWithUuidFields<T extends Record<string, unknown>>(
  statement: D1PreparedStatement,
  fields: string[],
) {
  const row = await statement.first<T>();
  return row ? decodeUuidFields(row, fields) : null;
}

async function allWithUuidFields<T extends Record<string, unknown>>(
  statement: D1PreparedStatement,
  fields: string[],
) {
  const result = await statement.all<T>();
  return result.results.map((row) => decodeUuidFields(row, fields));
}

function placeholders(count: number) {
  return Array.from({ length: count }, () => '?').join(', ');
}

export type SessionType = 'web' | 'device';

export async function findUserByEmail(db: D1Database, email: string) {
  const raw = await firstWithUuidFields<{
    id: string;
    email: string;
    password_hash: string;
    password_salt: ArrayBuffer;
    password_params_version: string;
    name: string | null;
    email_verified: number;
    email_bounced_at: number | null;
    settings: string;
    pub_key: ArrayBuffer | null;
    encrypted_priv_key: ArrayBuffer | null;
  }>(
    db
      .prepare(
        `SELECT id, email, password_hash, password_salt, password_params_version,
                name, email_verified, email_bounced_at, settings,
                pub_key, encrypted_priv_key
         FROM users
         WHERE email = ?`,
      )
      .bind(email),
    ['id'],
  );
  if (!raw) return null;
  return { ...raw, settings: parseUserSettings(raw.settings) };
}

export async function findUserById(db: D1Database, userId: string) {
  const raw = await firstWithUuidFields<{
    id: string;
    email: string;
    name: string | null;
    email_verified: number;
    email_bounced_at: number | null;
    settings: string;
    pub_key: ArrayBuffer | null;
    encrypted_priv_key: ArrayBuffer | null;
  }>(
    db
      .prepare(
        `SELECT id, email, name, email_verified, email_bounced_at, settings,
                pub_key, encrypted_priv_key
         FROM users
         WHERE id = ?`,
      )
      .bind(uuidToBytes(userId)),
    ['id'],
  );
  if (!raw) return null;
  return { ...raw, settings: parseUserSettings(raw.settings) };
}

export async function findUserPublicKeyByEmail(db: D1Database, email: string) {
  return firstWithUuidFields<{ id: string; pub_key: ArrayBuffer | null }>(
    db.prepare('SELECT id, pub_key FROM users WHERE email = ?').bind(email),
    ['id'],
  );
}

export async function markUsersUnverifiedByEmails(db: D1Database, emails: string[]) {
  const normalized = Array.from(
    new Set(emails.map((email) => email.trim().toLowerCase()).filter(Boolean)),
  );
  if (normalized.length === 0) {
    return null;
  }

  return db
    .prepare(
      `UPDATE users SET email_verified = 0 WHERE email IN (${placeholders(normalized.length)})`,
    )
    .bind(...normalized)
    .run();
}

export async function markUsersEmailBouncedByEmails(db: D1Database, emails: string[]) {
  const normalized = Array.from(
    new Set(emails.map((email) => email.trim().toLowerCase()).filter(Boolean)),
  );
  if (normalized.length === 0) {
    return null;
  }

  return db
    .prepare(
      `UPDATE users
       SET email_bounced_at = ?
       WHERE email IN (${placeholders(normalized.length)})`,
    )
    .bind(Date.now(), ...normalized)
    .run();
}

export type EmailBounceReason = 'permanent' | 'temporary';

// API-056
export async function createEmailBounce(
  db: D1Database,
  input: { id: string; email: string; bounced_at: number; reason: EmailBounceReason },
) {
  return db
    .prepare('INSERT INTO email_bounces (id, email, bounced_at, reason) VALUES (?, ?, ?, ?)')
    .bind(uuidToBytes(input.id), input.email, input.bounced_at, input.reason)
    .run();
}

export async function findPermanentEmailBounce(db: D1Database, email: string) {
  return firstWithUuidFields<{ id: string; bounced_at: number }>(
    db
      .prepare(
        `SELECT id, bounced_at FROM email_bounces
         WHERE email = ? AND reason = 'permanent'
         ORDER BY bounced_at DESC
         LIMIT 1`,
      )
      .bind(email.trim().toLowerCase()),
    ['id'],
  );
}

// Looks up the permanent bounces for many emails at once, for a bulk send that
// passes each recipient's to sendEmail (API-056). D1 allows at most 100 bound
// parameters per statement, so the emails go in statements of 100, all in one
// batch. Returns a lookup that gives an email's latest permanent bounce, or null.
export async function listPermanentEmailBounces(db: D1Database, emails: string[]) {
  const normalized = Array.from(
    new Set(emails.map((email) => email.trim().toLowerCase()).filter(Boolean)),
  );
  const chunks: string[][] = [];
  for (let i = 0; i < normalized.length; i += 100) {
    chunks.push(normalized.slice(i, i + 100));
  }

  const bounces = new Map<string, { id: string }>();
  if (chunks.length > 0) {
    const results = await db.batch<{ id: ArrayBuffer; email: string }>(
      chunks.map((chunk) =>
        db
          .prepare(
            `SELECT id, email FROM email_bounces
             WHERE reason = 'permanent' AND email IN (${placeholders(chunk.length)})
             ORDER BY bounced_at ASC`,
          )
          .bind(...chunk),
      ),
    );
    // Oldest first, so each email ends up with its latest bounce.
    for (const row of results.flatMap((result) => result.results)) {
      bounces.set(row.email, { id: bytesToUuid(row.id) });
    }
  }

  return (email: string) => bounces.get(email.trim().toLowerCase()) ?? null;
}

export async function deleteEmailBouncesByEmail(db: D1Database, email: string) {
  return db.prepare('DELETE FROM email_bounces WHERE email = ?').bind(email).run();
}

export async function setEmailTokenBounce(
  db: D1Database,
  input: { token_id: string; email?: string; bounce_id: string },
) {
  // The optional email guard keeps a bounce from landing on a token that was
  // sent to someone else.
  const query = input.email
    ? 'UPDATE email_tokens SET bounce_id = ? WHERE id = ? AND email = ?'
    : 'UPDATE email_tokens SET bounce_id = ? WHERE id = ?';
  const prepared = db.prepare(query);
  return (
    input.email
      ? prepared.bind(uuidToBytes(input.bounce_id), uuidToBytes(input.token_id), input.email)
      : prepared.bind(uuidToBytes(input.bounce_id), uuidToBytes(input.token_id))
  ).run();
}

export async function setBounceOnOutstandingEmailTokens(
  db: D1Database,
  email: string,
  bounceId: string,
) {
  return db
    .prepare(
      `UPDATE email_tokens SET bounce_id = ?
       WHERE email = ? AND consumed_at IS NULL AND bounce_id IS NULL`,
    )
    .bind(uuidToBytes(bounceId), email)
    .run();
}

export async function deleteEmailTokenById(db: D1Database, tokenId: string) {
  return db.prepare('DELETE FROM email_tokens WHERE id = ?').bind(uuidToBytes(tokenId)).run();
}

export async function createUser(
  db: D1Database,
  input: {
    id: string;
    email: string;
    passwordHash: string;
    passwordSalt: ArrayBuffer;
    passwordParamsVersion: string;
    pub_key: ArrayBuffer;
    encrypted_priv_key: ArrayBuffer;
    name?: string;
    email_verified?: boolean;
    timezone?: string;
  },
) {
  return db
    .prepare(
      `INSERT INTO users (
        id, email, password_hash, password_salt, password_params_version,
        name, email_verified, settings,
        pub_key, encrypted_priv_key, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(
      uuidToBytes(input.id),
      input.email,
      input.passwordHash,
      input.passwordSalt,
      input.passwordParamsVersion,
      input.name ?? null,
      input.email_verified ? 1 : 0,
      JSON.stringify({ email_frequency: 'weekly', timezone: input.timezone ?? 'UTC' }),
      input.pub_key,
      input.encrypted_priv_key,
      Date.now(),
    )
    .run();
}

export async function updateUser(
  db: D1Database,
  userId: string,
  fields: {
    email?: string;
    name?: string;
    password_hash?: string;
    password_salt?: ArrayBuffer;
    password_params_version?: string;
    email_verified?: boolean;
    email_bounced_at?: number | null;
    settings?: { email_frequency?: string; timezone?: string };
    pub_key?: ArrayBuffer;
    encrypted_priv_key?: ArrayBuffer;
  },
) {
  const updates: string[] = [];
  const params: (string | number | ArrayBuffer | null)[] = [];

  if (fields.email !== undefined) {
    updates.push('email = ?');
    params.push(fields.email);
  }

  if (fields.name !== undefined) {
    updates.push('name = ?');
    params.push(fields.name);
  }

  if (fields.password_hash !== undefined) {
    updates.push('password_hash = ?');
    params.push(fields.password_hash);
  }

  if (fields.password_salt !== undefined) {
    updates.push('password_salt = ?');
    params.push(fields.password_salt);
  }

  if (fields.password_params_version !== undefined) {
    updates.push('password_params_version = ?');
    params.push(fields.password_params_version);
  }

  if (fields.email_verified !== undefined) {
    updates.push('email_verified = ?');
    params.push(fields.email_verified ? 1 : 0);
  }

  if (fields.email_bounced_at !== undefined) {
    updates.push('email_bounced_at = ?');
    params.push(fields.email_bounced_at);
  }

  if (fields.settings !== undefined) {
    const patch: Record<string, string> = {};
    if (fields.settings.email_frequency !== undefined)
      patch.email_frequency = fields.settings.email_frequency;
    if (fields.settings.timezone !== undefined) patch.timezone = fields.settings.timezone;
    if (Object.keys(patch).length > 0) {
      updates.push('settings = json_patch(settings, ?)');
      params.push(JSON.stringify(patch));
    }
  }

  if (fields.pub_key !== undefined) {
    updates.push('pub_key = ?');
    params.push(fields.pub_key);
  }

  if (fields.encrypted_priv_key !== undefined) {
    updates.push('encrypted_priv_key = ?');
    params.push(fields.encrypted_priv_key);
  }

  if (updates.length === 0) {
    return null;
  }

  params.push(uuidToBytes(userId));
  return db
    .prepare(`UPDATE users SET ${updates.join(', ')} WHERE id = ?`)
    .bind(...params)
    .run();
}

// API-050: swaps the password and re-wrapped private key in one statement,
// conditioned on the hash the caller just verified so a concurrent change
// can't be silently overwritten. Returns false if that hash no longer matches.
// On success, revokes every other web session and any pending reset link.
export async function changeUserPassword(
  db: D1Database,
  userId: string,
  input: {
    expected_password_hash: string;
    password_hash: string;
    password_salt: ArrayBuffer;
    password_params_version: string;
    encrypted_priv_key: ArrayBuffer;
    keep_refresh_token_hash: string;
  },
) {
  const userIdBytes = uuidToBytes(userId);
  const result = await db
    .prepare(
      `UPDATE users
       SET password_hash = ?, password_salt = ?, password_params_version = ?, encrypted_priv_key = ?
       WHERE id = ? AND password_hash = ?`,
    )
    .bind(
      input.password_hash,
      input.password_salt,
      input.password_params_version,
      input.encrypted_priv_key,
      userIdBytes,
      input.expected_password_hash,
    )
    .run();

  if (result.meta.changes === 0) {
    return false;
  }

  await db.batch([
    db
      .prepare('DELETE FROM user_sessions WHERE user_id = ? AND refresh_token_hash != ?')
      .bind(userIdBytes, input.keep_refresh_token_hash),
    db
      .prepare(
        "UPDATE email_tokens SET consumed_at = ? WHERE user_id = ? AND purpose = 'password_reset' AND consumed_at IS NULL",
      )
      .bind(Date.now(), userIdBytes),
  ]);

  return true;
}

export async function createDevice(
  db: D1Database,
  input: { id: string; owner: string; name: string; platform: string },
) {
  return db
    .prepare('INSERT INTO devices (id, owner, name, platform, created_at) VALUES (?, ?, ?, ?, ?)')
    .bind(uuidToBytes(input.id), uuidToBytes(input.owner), input.name, input.platform, Date.now())
    .run();
}

export async function findDeviceById(db: D1Database, deviceId: string) {
  return firstWithUuidFields<{
    id: string;
    owner: string;
    name: string;
    platform: string;
    created_at: number;
  }>(
    db
      .prepare('SELECT id, owner, name, platform, created_at FROM devices WHERE id = ?')
      .bind(uuidToBytes(deviceId)),
    ['id', 'owner'],
  );
}

export async function findOwnedDevice(db: D1Database, deviceId: string, ownerId: string) {
  return firstWithUuidFields<{
    id: string;
    owner: string;
    name: string;
    platform: string;
    created_at: number;
  }>(
    db
      .prepare(
        'SELECT id, owner, name, platform, created_at FROM devices WHERE id = ? AND owner = ?',
      )
      .bind(uuidToBytes(deviceId), uuidToBytes(ownerId)),
    ['id', 'owner'],
  );
}

export async function updateDevice(db: D1Database, deviceId: string, fields: { name?: string }) {
  const updates: string[] = [];
  const params: SqlValue[] = [];

  if (fields.name !== undefined) {
    updates.push('name = ?');
    params.push(fields.name);
  }

  if (updates.length === 0) {
    return null;
  }

  params.push(uuidToBytes(deviceId));
  return db
    .prepare(`UPDATE devices SET ${updates.join(', ')} WHERE id = ?`)
    .bind(...params)
    .run();
}

export async function markDeviceDeleted(db: D1Database, deviceId: string, deletedAt: number) {
  return db
    .prepare('UPDATE devices SET deleted_at = ? WHERE id = ?')
    .bind(deletedAt, uuidToBytes(deviceId))
    .run();
}

export async function deleteDeviceSessionsByDeviceId(db: D1Database, deviceId: string) {
  return db
    .prepare('DELETE FROM device_sessions WHERE device_id = ?')
    .bind(uuidToBytes(deviceId))
    .run();
}

// batches has no index on device_id. These two name the owner as well, so they
// read the owner's primary key range instead of the whole table.
// batches.device_id has no foreign key, so nothing cascades from the device row:
// the device's batches are deleted here, in one transaction with the device. Returns
// the batches it deleted, so the caller can remove their R2 objects once that commits.
export async function deleteDeviceById(db: D1Database, deviceId: string, ownerId: string) {
  const deviceIdBytes = uuidToBytes(deviceId);
  const [deleted] = await db.batch([
    db
      .prepare(
        'DELETE FROM batches WHERE user_id = ? AND device_id = ? RETURNING id, user_id, version',
      )
      .bind(uuidToBytes(ownerId), deviceIdBytes),
    db.prepare('DELETE FROM devices WHERE id = ?').bind(deviceIdBytes),
  ]);
  return (deleted.results as { id: unknown; user_id: unknown; version: string }[]).map((row) => ({
    id: bytesToUuid(row.id),
    user_id: bytesToUuid(row.user_id),
    version: row.version,
  }));
}

export async function deleteUserById(db: D1Database, userId: string) {
  return db.prepare('DELETE FROM users WHERE id = ?').bind(uuidToBytes(userId)).run();
}

export async function listVisibleOwnerIds(db: D1Database, requesterId: string) {
  const partnerships = await allWithUuidFields<{ watching_user_id: string }>(
    db
      .prepare(
        "SELECT watching_user_id FROM partners WHERE watcher_user_id = ? AND status = 'accepted'",
      )
      .bind(uuidToBytes(requesterId)),
    ['watching_user_id'],
  );

  return [requesterId, ...partnerships.map((row) => row.watching_user_id)];
}

export async function listDevicesForOwners(db: D1Database, ownerIds: string[]) {
  if (ownerIds.length === 0) {
    return [];
  }

  return allWithUuidFields<{
    id: string;
    owner: string;
    name: string;
    platform: string;
    created_at: number;
    deleted_at: number | null;
  }>(
    db
      .prepare(
        // No batches here (API-029): a device's last upload is derived by the web
        // app from the batches /data already sends it, and status comes from the
        // hash server's last_received (hashGetMany in lib/hash-server.ts).
        `SELECT d.id, d.owner, d.name, d.platform, d.created_at, d.deleted_at
         FROM devices d
         WHERE d.owner IN (${placeholders(ownerIds.length)})
         ORDER BY d.created_at DESC`,
      )
      .bind(...ownerIds.map(uuidToBytes)),
    ['id', 'owner'],
  );
}

export async function createBatch(
  db: D1Database,
  input: {
    id: string;
    user_id: string;
    device_id: string;
    start_time: number;
    end_time: number;
    end_hash: ArrayBuffer;
    access_keys: Uint8Array;
    version: string;
    high_risk_count?: number;
    medium_risk_count?: number;
    created_at: number;
  },
) {
  const deviceIdBytes = uuidToBytes(input.device_id);
  return db
    .prepare(
      // batches.device_id has no foreign key, so the insert checks the device still
      // exists itself; otherwise a device deleted mid-upload would leave an orphan row.
      `INSERT INTO batches (
         id, user_id, device_id, start_time, end_time, end_hash, access_keys,
         version, high_risk_count, medium_risk_count, created_at
       )
       SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
       WHERE EXISTS (SELECT 1 FROM devices WHERE id = ?)`,
    )
    .bind(
      uuidToBytes(input.id),
      uuidToBytes(input.user_id),
      deviceIdBytes,
      input.start_time,
      input.end_time,
      input.end_hash,
      input.access_keys,
      input.version,
      input.high_risk_count ?? 0,
      input.medium_risk_count ?? 0,
      input.created_at,
      deviceIdBytes,
    )
    .run();
}

export async function listBatches(db: D1Database, ownerIds: string[], filters: { since?: number }) {
  if (ownerIds.length === 0) {
    return [];
  }

  const params: SqlValue[] = ownerIds.map(uuidToBytes);
  let query = `SELECT id, user_id, device_id, start_time, end_time, end_hash, access_keys, version, created_at
               FROM batches
               WHERE user_id IN (${placeholders(ownerIds.length)})`;

  if (filters.since !== undefined) {
    query += ' AND created_at > ?';
    params.push(filters.since);
  }

  query += ' ORDER BY created_at ASC';

  return allWithUuidFields<{
    id: string;
    user_id: string;
    device_id: string;
    start_time: number;
    end_time: number;
    // BLOBs. access_keys is still JSON text on rows from before it became msgpack.
    end_hash: unknown;
    access_keys: unknown;
    version: string;
    created_at: number;
  }>(db.prepare(query).bind(...params), ['id', 'user_id', 'device_id']);
}

// The next ?2 user ids after ?1 that have batches, in id order. SQLite can't list the
// distinct values of an index's first column without reading every row, so each step
// seeks to the first batch past the previous user, one row read per user. Users without
// batches, who haven't uploaded in 30 days, are never visited.
const NEXT_USERS_WITH_BATCHES = `WITH RECURSIVE next_user(id, n) AS (
  SELECT (SELECT user_id FROM batches WHERE user_id > ?1 ORDER BY user_id LIMIT 1), 1
  UNION ALL
  SELECT (SELECT user_id FROM batches WHERE user_id > next_user.id ORDER BY user_id LIMIT 1), n + 1
  FROM next_user WHERE next_user.id IS NOT NULL AND n < ?2
)`;

// Deletes the expired batches of the next `userLimit` users after `afterUserId` that have
// any, and returns the id to continue after (null once every user is covered).
export async function deleteExpiredBatchesForUsers(
  db: D1Database,
  afterUserId: Uint8Array,
  userLimit: number,
  cutoff: number,
) {
  const [next, deleted] = await db.batch([
    // Taken before the delete, which can remove a user's last batch.
    db
      .prepare(`${NEXT_USERS_WITH_BATCHES} SELECT max(id) AS id, count(id) AS users FROM next_user`)
      .bind(afterUserId, userLimit),
    db
      .prepare(
        // batches has no created_at index, since one cost a written row per batch.
        // Each user's batches sit in created_at order under the (user_id, created_at,
        // id) primary key, so their expired ones are the front of that range and the
        // delete seeks straight to them.
        `${NEXT_USERS_WITH_BATCHES}
         DELETE FROM batches WHERE user_id IN (SELECT id FROM next_user WHERE id IS NOT NULL)
           AND created_at < ?3`,
      )
      .bind(afterUserId, userLimit, cutoff),
  ]);
  const row = (next.results as { id: ArrayBuffer | number[] | null; users: number }[])[0];
  return {
    deleted: deleted.meta.changes,
    nextUserId:
      row?.id && row.users === userLimit
        ? row.id instanceof ArrayBuffer
          ? new Uint8Array(row.id)
          : Uint8Array.from(row.id)
        : null,
  };
}

export async function deleteExpiredEmailTokensChunk(db: D1Database, cutoff: number, limit: number) {
  const result = await db
    .prepare(
      `DELETE FROM email_tokens WHERE id IN (
         SELECT id FROM email_tokens WHERE expires_at < ? ORDER BY expires_at ASC LIMIT ?
       )`,
    )
    .bind(cutoff, limit)
    .run();
  return result.meta.changes;
}

export async function deleteExpiredUserSessionsChunk(
  db: D1Database,
  cutoff: number,
  limit: number,
) {
  const result = await db
    .prepare(
      `DELETE FROM user_sessions WHERE refresh_token_hash IN (
         SELECT refresh_token_hash FROM user_sessions WHERE expires_at < ? ORDER BY expires_at ASC LIMIT ?
       )`,
    )
    .bind(cutoff, limit)
    .run();
  return result.meta.changes;
}

export async function deleteExpiredDeviceSessionsChunk(
  db: D1Database,
  cutoff: number,
  limit: number,
) {
  const result = await db
    .prepare(
      `DELETE FROM device_sessions WHERE refresh_token_hash IN (
         SELECT refresh_token_hash FROM device_sessions WHERE expires_at < ? ORDER BY expires_at ASC LIMIT ?
       )`,
    )
    .bind(cutoff, limit)
    .run();
  return result.meta.changes;
}

// D1 allows at most 100 bound parameters per statement, and the digest
// statements bind up to 10 besides the user IDs.
const DIGEST_ACTIVITY_CHUNK = 90;
// Margin on listDigestActivity's created_at bound. POST /d/batch clamps
// end_time to created_at (API-038), so new batches need none; it covers rows
// stored before that clamp, from devices whose clock ran ahead.
const BATCH_CLOCK_SKEW_MS = 24 * 60 * 60 * 1000;

const DIGEST_DAY_MS = 24 * 60 * 60 * 1000;

export type DigestUserTotals = {
  user_id: string;
  screenshots: number;
  critical: number;
  warning: number;
};

export type DigestDeviceDays = {
  id: string;
  owner: string;
  name: string;
  created_at: number;
  // A run of days the device has batches on, or both null for a device with
  // none. Day 0 starts at the later of the window start and created_at.
  first_day: number | null;
  last_day: number | null;
};

/**
 * Sums up, in SQL, what digests need about a set of watched users over one
 * window (API-057), so no batch rows come back however many there are:
 *
 * - per user, the approximate screenshot count (each batch counts its overlap
 *   with the window in capture intervals, rounded, and at least 1) and the
 *   batches' high and medium risk counts;
 * - per device, the days it has batches on. Days are counted from the later of
 *   the window start and the device's created_at, which is how a digest reports
 *   missing days, and a batch covers every day it overlaps.
 *
 * Every statement goes in a single D1 batch, so one round trip covers all the users.
 */
export async function listDigestActivity(
  db: D1Database,
  userIds: string[],
  window: { start: number; end: number },
  captureIntervalMs: number,
) {
  const chunks: ArrayBuffer[][] = [];
  for (let i = 0; i < userIds.length; i += DIGEST_ACTIVITY_CHUNK) {
    chunks.push(userIds.slice(i, i + DIGEST_ACTIVITY_CHUNK).map(uuidToBytes));
  }
  if (chunks.length === 0) {
    return { totals: [], devices: [] };
  }
  const createdAfter = window.start - BATCH_CLOCK_SKEW_MS;

  const results = await db.batch<Record<string, unknown>>([
    ...chunks.map((ids) =>
      db
        .prepare(
          // A batch is stored after the period it covers ends, and POST /d/batch clamps
          // end_time to the time it was stored, so created_at >= end_time. The created_at
          // bound lets the (user_id, created_at, id) primary key skip everything well
          // before the window instead of reading each user's whole retention period.
          `SELECT user_id,
                  CAST(SUM(MAX(1, ROUND((MIN(end_time, ?) - MAX(start_time, ?)) * 1.0 / ?)))
                       AS INTEGER) AS screenshots,
                  SUM(high_risk_count) AS critical,
                  SUM(medium_risk_count) AS warning
           FROM batches
           WHERE user_id IN (${placeholders(ids.length)})
             AND created_at > ? AND end_time > ? AND start_time < ?
           GROUP BY user_id`,
        )
        .bind(
          window.end,
          window.start,
          captureIntervalMs,
          ...ids,
          createdAfter,
          window.start,
          window.end,
        ),
    ),
    ...chunks.map((ids) =>
      db
        .prepare(
          // first_day and last_day are the first and last whole days, counted from
          // MAX(window start, d.created_at), that the batch overlaps. A device with no
          // batches in its part of the window gets one row with both null.
          `SELECT d.id, d.owner, d.name, d.created_at,
                  CAST((MAX(b.start_time, MAX(?, d.created_at)) - MAX(?, d.created_at)) / ?
                       AS INTEGER) AS first_day,
                  CAST((MIN(b.end_time, ?) - 1 - MAX(?, d.created_at)) / ?
                       AS INTEGER) AS last_day
           FROM devices d
           LEFT JOIN batches b
             ON b.user_id = d.owner AND b.device_id = d.id
            AND b.created_at > ? AND b.end_time > MAX(?, d.created_at) AND b.start_time < ?
           WHERE d.owner IN (${placeholders(ids.length)})
           GROUP BY d.id, first_day, last_day
           ORDER BY d.created_at DESC, d.id, first_day`,
        )
        .bind(
          window.start,
          window.start,
          DIGEST_DAY_MS,
          window.end,
          window.start,
          DIGEST_DAY_MS,
          createdAfter,
          window.start,
          window.end,
          ...ids,
        ),
    ),
  ]);

  const totalResults = results.slice(0, chunks.length);
  const deviceResults = results.slice(chunks.length);
  return {
    totals: totalResults.flatMap((result) =>
      result.results.map((row) =>
        decodeUuidFields(row as unknown as DigestUserTotals, ['user_id']),
      ),
    ),
    devices: deviceResults.flatMap((result) =>
      result.results.map((row) =>
        decodeUuidFields(row as unknown as DigestDeviceDays, ['id', 'owner']),
      ),
    ),
  };
}

export async function findPartnerInviteForOwner(db: D1Database, ownerId: string, email: string) {
  return firstWithUuidFields<{ id: string }>(
    db
      .prepare('SELECT id FROM partners WHERE watching_user_id = ? AND watcher_email = ?')
      .bind(uuidToBytes(ownerId), email),
    ['id'],
  );
}

export async function createPartner(
  db: D1Database,
  input: {
    id: string;
    watching_user_id: string;
    watcher_email: string;
    invite_token_id: string;
    created_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO partners (
        id, watching_user_id, watcher_user_id, watcher_email, invite_token_id, status, created_at, updated_at
      ) VALUES (?, ?, NULL, ?, ?, 'pending', ?, ?)`,
    )
    .bind(
      uuidToBytes(input.id),
      uuidToBytes(input.watching_user_id),
      input.watcher_email,
      uuidToBytes(input.invite_token_id),
      input.created_at,
      input.created_at,
    )
    .run();
}

export async function findPartnerById(db: D1Database, partnerId: string) {
  return firstWithUuidFields<{
    id: string;
    watching_user_id: string;
    watcher_user_id: string | null;
    watcher_email: string;
    invite_token_id: string | null;
    invite_expires_at: number | null;
    invite_consumed_at: number | null;
    status: string;
    created_at: number;
    updated_at: number;
  }>(
    db
      .prepare(
        `SELECT p.id,
                p.watching_user_id,
                p.watcher_user_id,
                p.watcher_email,
                p.invite_token_id,
                et.expires_at AS invite_expires_at,
                et.consumed_at AS invite_consumed_at,
                p.status,
                p.created_at,
                p.updated_at
         FROM partners p
         LEFT JOIN email_tokens et ON et.id = p.invite_token_id
         WHERE p.id = ?`,
      )
      .bind(uuidToBytes(partnerId)),
    ['id', 'watching_user_id', 'watcher_user_id', 'invite_token_id'],
  );
}

export async function findPartnerByInviteTokenHash(db: D1Database, tokenHash: string) {
  return firstWithUuidFields<{
    id: string;
    watching_user_id: string;
    watcher_user_id: string | null;
    watcher_email: string;
    invite_token_id: string | null;
    invite_expires_at: number | null;
    invite_consumed_at: number | null;
    status: string;
    created_at: number;
    updated_at: number;
  }>(
    db
      .prepare(
        `SELECT p.id,
                p.watching_user_id,
                p.watcher_user_id,
                p.watcher_email,
                p.invite_token_id,
                et.expires_at AS invite_expires_at,
                et.consumed_at AS invite_consumed_at,
                p.status,
                p.created_at,
                p.updated_at
         FROM partners p
         JOIN email_tokens et ON et.id = p.invite_token_id
         WHERE et.token_hash = ? AND et.purpose = 'partner_invite'`,
      )
      .bind(tokenHash),
    ['id', 'watching_user_id', 'watcher_user_id', 'invite_token_id'],
  );
}

export async function findPartnerForOwnerAndUser(
  db: D1Database,
  ownerId: string,
  partnerUserId: string,
  excludeId?: string,
) {
  const query = excludeId
    ? 'SELECT id FROM partners WHERE watching_user_id = ? AND watcher_user_id = ? AND id != ?'
    : 'SELECT id FROM partners WHERE watching_user_id = ? AND watcher_user_id = ?';
  const prepared = db.prepare(query);
  return firstWithUuidFields<{ id: string }>(
    excludeId
      ? prepared.bind(uuidToBytes(ownerId), uuidToBytes(partnerUserId), uuidToBytes(excludeId))
      : prepared.bind(uuidToBytes(ownerId), uuidToBytes(partnerUserId)),
    ['id'],
  );
}

export async function acceptPartner(
  db: D1Database,
  input: { id: string; watcherUserId: string; watcherEmail: string; updated_at: number },
) {
  return db
    .prepare(
      `UPDATE partners
       SET watcher_user_id = ?, watcher_email = ?,
           status = 'accepted', accepted_at = ?, updated_at = ?
       WHERE id = ?`,
    )
    .bind(
      uuidToBytes(input.watcherUserId),
      input.watcherEmail,
      input.updated_at,
      input.updated_at,
      uuidToBytes(input.id),
    )
    .run();
}

export async function listOwnedPartners(db: D1Database, ownerId: string) {
  return allWithUuidFields<{
    id: string;
    status: string;
    created_at: number;
    accepted_at: number | null;
    watcher_email: string;
    watcher_id: string | null;
    watcher_name: string | null;
    invite_bounced: number;
  }>(
    db
      .prepare(
        `SELECT p.id, p.status, p.created_at, p.accepted_at, p.watcher_email,
                 u.id AS watcher_id, u.name AS watcher_name,
                 et.bounce_id IS NOT NULL AS invite_bounced
           FROM partners p
           LEFT JOIN users u ON u.id = p.watcher_user_id
           LEFT JOIN email_tokens et ON et.id = p.invite_token_id
           WHERE p.watching_user_id = ?
           ORDER BY p.created_at DESC`,
      )
      .bind(uuidToBytes(ownerId)),
    ['id', 'watcher_id'],
  );
}

export async function listIncomingPartners(db: D1Database, partnerUserId: string) {
  return allWithUuidFields<{
    id: string;
    status: string;
    created_at: number;
    accepted_at: number | null;
    watching_user_id: string;
    watching_user_email: string;
    watching_user_name: string | null;
  }>(
    db
      .prepare(
        `SELECT p.id, p.status, p.created_at, p.accepted_at,
                u.id AS watching_user_id, u.email AS watching_user_email, u.name AS watching_user_name
          FROM partners p
          JOIN users u ON u.id = p.watching_user_id
          WHERE p.watcher_user_id = ?
          ORDER BY p.created_at DESC`,
      )
      .bind(uuidToBytes(partnerUserId)),
    ['id', 'watching_user_id'],
  );
}

export async function deletePartnerById(db: D1Database, partnerId: string) {
  return db.prepare('DELETE FROM partners WHERE id = ?').bind(uuidToBytes(partnerId)).run();
}

export async function listBatchAccessRecipientsForOwner(db: D1Database, ownerId: string) {
  const [owner, partners] = await Promise.all([
    firstWithUuidFields<{ id: string; pub_key: ArrayBuffer | null }>(
      db.prepare('SELECT id, pub_key FROM users WHERE id = ?').bind(uuidToBytes(ownerId)),
      ['id'],
    ),
    allWithUuidFields<{ id: string; pub_key: ArrayBuffer | null }>(
      db
        .prepare(
          `SELECT recipient.id AS id, recipient.pub_key
           FROM partners p
           JOIN users recipient ON recipient.id = p.watcher_user_id
           WHERE p.watching_user_id = ? AND p.status = 'accepted'`,
        )
        .bind(uuidToBytes(ownerId)),
      ['id'],
    ),
  ]);

  return [
    ...(owner?.pub_key ? [{ id: owner.id, pub_key: owner.pub_key }] : []),
    ...partners.filter((partner) => partner.pub_key),
  ];
}

export async function listAcceptedNotificationTargetsForUser(db: D1Database, userId: string) {
  const rows = await allWithUuidFields<{
    partnership_id: string;
    watcher_email: string;
    watcher_user_id: string | null;
    watcher_name: string | null;
    watcher_email_verified: number;
    settings: string;
  }>(
    db
      .prepare(
        `SELECT p.id AS partnership_id,
                recipient.email AS watcher_email,
                recipient.id AS watcher_user_id,
                recipient.name AS watcher_name,
                recipient.email_verified AS watcher_email_verified,
                recipient.settings
          FROM partners p
          JOIN users recipient ON recipient.id = p.watcher_user_id
          WHERE p.watching_user_id = ? AND p.status = 'accepted'`,
      )
      .bind(uuidToBytes(userId)),
    ['partnership_id', 'watcher_user_id'],
  );
  return rows.map((row) => ({ ...row, settings: parseUserSettings(row.settings) }));
}

export async function createEmailToken(
  db: D1Database,
  input: {
    id: string;
    user_id?: string | null;
    email: string;
    purpose: string;
    token_hash: string;
    expires_at: number;
    created_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO email_tokens (id, user_id, email, purpose, token_hash, expires_at, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(
      uuidToBytes(input.id),
      input.user_id ? uuidToBytes(input.user_id) : null,
      input.email,
      input.purpose,
      input.token_hash,
      input.expires_at,
      input.created_at,
    )
    .run();
}

export async function invalidateEmailTokens(db: D1Database, userId: string, purpose: string) {
  return db
    .prepare(
      'UPDATE email_tokens SET consumed_at = ? WHERE user_id = ? AND purpose = ? AND consumed_at IS NULL',
    )
    .bind(Date.now(), uuidToBytes(userId), purpose)
    .run();
}

export async function findEmailTokenByHash(db: D1Database, tokenHash: string, purpose?: string) {
  const query = purpose
    ? `SELECT id, user_id, email, purpose, token_hash, expires_at, consumed_at, created_at
       FROM email_tokens
       WHERE token_hash = ? AND purpose = ?`
    : `SELECT id, user_id, email, purpose, token_hash, expires_at, consumed_at, created_at
       FROM email_tokens
       WHERE token_hash = ?`;
  const prepared = db.prepare(query);
  return firstWithUuidFields<{
    id: string;
    user_id: string | null;
    email: string;
    purpose: string;
    token_hash: string;
    expires_at: number;
    consumed_at: number | null;
    created_at: number;
  }>(purpose ? prepared.bind(tokenHash, purpose) : prepared.bind(tokenHash), ['id', 'user_id']);
}

export async function consumeEmailToken(
  db: D1Database,
  token: { id: string; user_id: string | null; purpose: string },
  consumedAt: number,
) {
  await db
    .prepare('UPDATE email_tokens SET consumed_at = ? WHERE id = ? AND consumed_at IS NULL')
    .bind(consumedAt, uuidToBytes(token.id))
    .run();
  if (token.user_id) {
    await invalidateEmailTokens(db, token.user_id, token.purpose);
  }
}

export async function createSessionRecord(
  db: D1Database,
  input: {
    session_type: SessionType;
    user_id?: string;
    device_id?: string;
    refresh_token_hash: string;
    expires_at: number;
    created_at: number;
  },
) {
  if (input.session_type === 'web') {
    return db
      .prepare(
        `INSERT INTO user_sessions (refresh_token_hash, user_id, expires_at, created_at)
         VALUES (?, ?, ?, ?)`,
      )
      .bind(
        input.refresh_token_hash,
        uuidToBytes(input.user_id!),
        input.expires_at,
        input.created_at,
      )
      .run();
  }

  return db
    .prepare(
      `INSERT INTO device_sessions (refresh_token_hash, device_id, expires_at, created_at)
       VALUES (?, ?, ?, ?)`,
    )
    .bind(
      input.refresh_token_hash,
      uuidToBytes(input.device_id!),
      input.expires_at,
      input.created_at,
    )
    .run();
}

export async function findSessionByRefreshTokenHash(
  db: D1Database,
  refreshTokenHash: string,
  sessionType: SessionType,
) {
  if (sessionType === 'web') {
    return firstWithUuidFields<{
      user_id: string;
      device_id: null;
      refresh_token_hash: string;
      expires_at: number;
      created_at: number;
    }>(
      db
        .prepare(
          `SELECT user_id, NULL AS device_id, refresh_token_hash, expires_at, created_at
           FROM user_sessions
           WHERE refresh_token_hash = ?`,
        )
        .bind(refreshTokenHash),
      ['user_id'],
    );
  }

  return firstWithUuidFields<{
    user_id: null;
    device_id: string;
    refresh_token_hash: string;
    expires_at: number;
    created_at: number;
  }>(
    db
      .prepare(
        `SELECT NULL AS user_id, device_id, refresh_token_hash, expires_at, created_at
         FROM device_sessions
         WHERE refresh_token_hash = ?`,
      )
      .bind(refreshTokenHash),
    ['device_id'],
  );
}

export async function deleteSessionByRefreshTokenHash(
  db: D1Database,
  refreshTokenHash: string,
  sessionType?: SessionType,
) {
  if (sessionType === 'web') {
    return db
      .prepare('DELETE FROM user_sessions WHERE refresh_token_hash = ?')
      .bind(refreshTokenHash)
      .run();
  }

  if (sessionType === 'device') {
    return db
      .prepare('DELETE FROM device_sessions WHERE refresh_token_hash = ?')
      .bind(refreshTokenHash)
      .run();
  }

  await db
    .prepare('DELETE FROM user_sessions WHERE refresh_token_hash = ?')
    .bind(refreshTokenHash)
    .run();
  return db
    .prepare('DELETE FROM device_sessions WHERE refresh_token_hash = ?')
    .bind(refreshTokenHash)
    .run();
}

export async function createLockedPassword(
  db: D1Database,
  input: { id: string; owner_id: string; label: string; wrapped_value: string; created_at: number },
) {
  return db
    .prepare(
      `INSERT INTO locked_passwords (id, owner_id, label, wrapped_value, created_at)
       VALUES (?, ?, ?, ?, ?)`,
    )
    .bind(
      uuidToBytes(input.id),
      uuidToBytes(input.owner_id),
      input.label,
      input.wrapped_value,
      input.created_at,
    )
    .run();
}

export async function findOwnedLockedPassword(db: D1Database, id: string, ownerId: string) {
  return firstWithUuidFields<{
    id: string;
    owner_id: string;
    label: string;
    wrapped_value: string;
    accessed_at: number | null;
    deleted_at: number | null;
    created_at: number;
  }>(
    db
      .prepare(
        `SELECT id, owner_id, label, wrapped_value, accessed_at, deleted_at, created_at
         FROM locked_passwords
         WHERE id = ? AND owner_id = ?`,
      )
      .bind(uuidToBytes(id), uuidToBytes(ownerId)),
    ['id', 'owner_id'],
  );
}

export async function listLockedPasswordsForOwner(db: D1Database, ownerId: string) {
  return allWithUuidFields<{
    id: string;
    label: string;
    accessed_at: number | null;
    deleted_at: number | null;
    created_at: number;
  }>(
    db
      .prepare(
        `SELECT id, label, accessed_at, deleted_at, created_at
         FROM locked_passwords
         WHERE owner_id = ?
         ORDER BY created_at DESC`,
      )
      .bind(uuidToBytes(ownerId)),
    ['id'],
  );
}

export async function markLockedPasswordAccessed(db: D1Database, id: string, accessedAt: number) {
  return db
    .prepare('UPDATE locked_passwords SET accessed_at = ? WHERE id = ? AND accessed_at IS NULL')
    .bind(accessedAt, uuidToBytes(id))
    .run();
}

export async function markLockedPasswordDeleted(db: D1Database, id: string, deletedAt: number) {
  return db
    .prepare('UPDATE locked_passwords SET deleted_at = ? WHERE id = ?')
    .bind(deletedAt, uuidToBytes(id))
    .run();
}

export async function restoreLockedPassword(db: D1Database, id: string) {
  return db
    .prepare('UPDATE locked_passwords SET deleted_at = NULL WHERE id = ?')
    .bind(uuidToBytes(id))
    .run();
}

export async function deleteLockedPasswordById(db: D1Database, id: string) {
  return db.prepare('DELETE FROM locked_passwords WHERE id = ?').bind(uuidToBytes(id)).run();
}

export async function deleteExpiredLockedPasswordsChunk(
  db: D1Database,
  cutoff: number,
  limit: number,
) {
  const result = await db
    .prepare(
      `DELETE FROM locked_passwords WHERE id IN (
         SELECT id FROM locked_passwords
         WHERE deleted_at IS NOT NULL AND deleted_at < ?
         ORDER BY deleted_at ASC LIMIT ?
       )`,
    )
    .bind(cutoff, limit)
    .run();
  return result.meta.changes;
}

// Unverified watchers are left out, since sendEmail would skip their digest
// anyway (API-057), and this saves building it. watcher_email_verified is still
// returned so sendEmail can check it without looking the watcher up again.
export async function listDigestEligiblePartnerships(db: D1Database) {
  const rows = await allWithUuidFields<{
    partnership_id: string;
    watching_user_id: string;
    watcher_user_id: string;
    watcher_email: string;
    watcher_email_verified: number;
    watching_user_email: string;
    watching_user_name: string | null;
    settings: string;
  }>(
    db.prepare(
      `SELECT p.id AS partnership_id,
              p.watching_user_id,
              recipient.id AS watcher_user_id,
              recipient.email AS watcher_email,
              recipient.email_verified AS watcher_email_verified,
              recipient.settings,
              owner.email AS watching_user_email,
              owner.name AS watching_user_name
        FROM partners p
        JOIN users owner ON owner.id = p.watching_user_id
        JOIN users recipient ON recipient.id = p.watcher_user_id
        WHERE p.status = 'accepted' AND recipient.email_verified = 1`,
    ),
    ['partnership_id', 'watching_user_id', 'watcher_user_id'],
  );
  return rows.map((row) => ({ ...row, settings: parseUserSettings(row.settings) }));
}

// api/SPEC.md API-051: admin status is read from the admins table only; nothing
// in this module writes to it.
export async function isAdmin(db: D1Database, userId: string) {
  const row = await db
    .prepare('SELECT 1 AS present FROM admins WHERE user_id = ?')
    .bind(uuidToBytes(userId))
    .first<{ present: number }>();
  return row !== null;
}

export async function upsertAnalyticsSnapshot(
  db: D1Database,
  input: { day: string; metrics: string; created_at: number },
) {
  return db
    .prepare(
      `INSERT OR REPLACE INTO analytics_snapshots (day, metrics, created_at)
       VALUES (?, ?, ?)`,
    )
    .bind(input.day, input.metrics, input.created_at)
    .run();
}

export async function listAnalyticsSnapshots(db: D1Database, limit: number) {
  const result = await db
    .prepare(
      `SELECT day, metrics, created_at
       FROM analytics_snapshots
       ORDER BY day DESC
       LIMIT ?`,
    )
    .bind(limit)
    .all<{ day: string; metrics: string; created_at: number }>();
  return result.results;
}
