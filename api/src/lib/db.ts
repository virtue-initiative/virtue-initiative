type SqlValue = string | number | ArrayBuffer | null;

function uuidToBytes(uuid: string): ArrayBuffer {
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

function bytesToUuid(value: unknown): string {
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

export type PartnerPermissions = {
  view_data?: boolean;
};

export type SessionType = 'web' | 'device';

export function parsePermissions(value: string): PartnerPermissions {
  return JSON.parse(value) as PartnerPermissions;
}

export async function findUserByEmail(db: D1Database, email: string) {
  return firstWithUuidFields<{
    id: string;
    email: string;
    password_hash: string;
    name: string | null;
    email_verified: number;
    e2ee_key: ArrayBuffer | null;
    pub_key: ArrayBuffer | null;
    priv_key: ArrayBuffer | null;
  }>(
    db
      .prepare(
        'SELECT id, email, password_hash, name, email_verified, e2ee_key, pub_key, priv_key FROM users WHERE email = ?',
      )
      .bind(email),
    ['id'],
  );
}

export async function findUserById(db: D1Database, userId: string) {
  return firstWithUuidFields<{
    id: string;
    email: string;
    name: string | null;
    email_verified: number;
    e2ee_key: ArrayBuffer | null;
    pub_key: ArrayBuffer | null;
    priv_key: ArrayBuffer | null;
  }>(
    db
      .prepare(
        'SELECT id, email, name, email_verified, e2ee_key, pub_key, priv_key FROM users WHERE id = ?',
      )
      .bind(uuidToBytes(userId)),
    ['id'],
  );
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
      `UPDATE users SET email_verified = 0 WHERE lower(email) IN (${placeholders(normalized.length)})`,
    )
    .bind(...normalized)
    .run();
}

export async function createUser(
  db: D1Database,
  input: { id: string; email: string; passwordHash: string; name?: string },
) {
  return db
    .prepare(
      'INSERT INTO users (id, email, password_hash, name, email_verified, created_at) VALUES (?, ?, ?, ?, 0, ?)',
    )
    .bind(uuidToBytes(input.id), input.email, input.passwordHash, input.name ?? null, Date.now())
    .run();
}

export async function updateUser(
  db: D1Database,
  userId: string,
  fields: {
    name?: string;
    password_hash?: string;
    email_verified?: boolean;
    e2ee_key?: ArrayBuffer;
    pub_key?: ArrayBuffer;
    priv_key?: ArrayBuffer;
  },
) {
  const updates: string[] = [];
  const params: (string | number | ArrayBuffer | null)[] = [];

  if (fields.name !== undefined) {
    updates.push('name = ?');
    params.push(fields.name);
  }

  if (fields.password_hash !== undefined) {
    updates.push('password_hash = ?');
    params.push(fields.password_hash);
  }

  if (fields.email_verified !== undefined) {
    updates.push('email_verified = ?');
    params.push(fields.email_verified ? 1 : 0);
  }

  if (fields.e2ee_key !== undefined) {
    updates.push('e2ee_key = ?');
    params.push(fields.e2ee_key);
  }

  if (fields.pub_key !== undefined) {
    updates.push('pub_key = ?');
    params.push(fields.pub_key);
  }

  if (fields.priv_key !== undefined) {
    updates.push('priv_key = ?');
    params.push(fields.priv_key);
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

export async function createDevice(
  db: D1Database,
  input: { id: string; owner: string; name: string; platform: string },
) {
  return db
    .prepare(
      'INSERT INTO devices (id, owner, name, platform, enabled, created_at) VALUES (?, ?, ?, ?, 1, ?)',
    )
    .bind(uuidToBytes(input.id), uuidToBytes(input.owner), input.name, input.platform, Date.now())
    .run();
}

export async function findDeviceById(db: D1Database, deviceId: string) {
  return firstWithUuidFields<{
    id: string;
    owner: string;
    name: string;
    platform: string;
    enabled: number;
    created_at: number;
  }>(
    db
      .prepare('SELECT id, owner, name, platform, enabled, created_at FROM devices WHERE id = ?')
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
    enabled: number;
    created_at: number;
  }>(
    db
      .prepare(
        'SELECT id, owner, name, platform, enabled, created_at FROM devices WHERE id = ? AND owner = ?',
      )
      .bind(uuidToBytes(deviceId), uuidToBytes(ownerId)),
    ['id', 'owner'],
  );
}

export async function updateDevice(
  db: D1Database,
  deviceId: string,
  fields: { name?: string; enabled?: boolean },
) {
  const updates: string[] = [];
  const params: SqlValue[] = [];

  if (fields.name !== undefined) {
    updates.push('name = ?');
    params.push(fields.name);
  }

  if (fields.enabled !== undefined) {
    updates.push('enabled = ?');
    params.push(fields.enabled ? 1 : 0);
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

export async function listBatchUrlsForDevice(db: D1Database, deviceId: string) {
  const result = await db
    .prepare('SELECT url FROM batches WHERE device_id = ?')
    .bind(uuidToBytes(deviceId))
    .all<{ url: string }>();

  return result.results;
}

export async function deleteDeviceById(db: D1Database, deviceId: string) {
  const deviceIdBytes = uuidToBytes(deviceId);
  await db.prepare('DELETE FROM device_logs WHERE device_id = ?').bind(deviceIdBytes).run();
  await db.prepare('DELETE FROM batches WHERE device_id = ?').bind(deviceIdBytes).run();
  await db.prepare('DELETE FROM hash_states WHERE device_id = ?').bind(deviceIdBytes).run();
  return db.prepare('DELETE FROM devices WHERE id = ?').bind(deviceIdBytes).run();
}

export async function listVisibleOwnerIds(db: D1Database, requesterId: string) {
  const partnerships = await allWithUuidFields<{ user_id: string; permissions: string }>(
    db
      .prepare(
        "SELECT user_id, permissions FROM partners WHERE partner_user_id = ? AND status = 'accepted'",
      )
      .bind(uuidToBytes(requesterId)),
    ['user_id'],
  );

  return [
    requesterId,
    ...partnerships
      .filter((row) => parsePermissions(row.permissions).view_data)
      .map((row) => row.user_id),
  ];
}

export async function canViewUserData(db: D1Database, ownerId: string, requesterId: string) {
  if (ownerId === requesterId) return true;

  const partnership = await db
    .prepare(
      "SELECT permissions FROM partners WHERE user_id = ? AND partner_user_id = ? AND status = 'accepted'",
    )
    .bind(uuidToBytes(ownerId), uuidToBytes(requesterId))
    .first<{ permissions: string }>();

  return partnership ? Boolean(parsePermissions(partnership.permissions).view_data) : false;
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
    enabled: number;
    created_at: number;
    last_upload_at: number | null;
  }>(
    db
      .prepare(
        `SELECT d.id, d.owner, d.name, d.platform, d.enabled, d.created_at, MAX(b.end) AS last_upload_at
         FROM devices d
         LEFT JOIN batches b ON b.device_id = d.id
         WHERE d.owner IN (${placeholders(ownerIds.length)})
         GROUP BY d.id
         ORDER BY d.created_at DESC`,
      )
      .bind(...ownerIds.map(uuidToBytes)),
    ['id', 'owner'],
  );
}

export async function listEnabledDevicesWithLastUpload(db: D1Database) {
  return allWithUuidFields<{
    id: string;
    owner: string;
    name: string;
    platform: string;
    enabled: number;
    created_at: number;
    last_upload_at: number | null;
    owner_email: string;
    owner_name: string | null;
  }>(
    db.prepare(
      `SELECT d.id, d.owner, d.name, d.platform, d.enabled, d.created_at, MAX(b.end) AS last_upload_at,
              u.email AS owner_email, u.name AS owner_name
       FROM devices d
       JOIN users u ON u.id = d.owner
       LEFT JOIN batches b ON b.device_id = d.id
       WHERE d.enabled = 1
       GROUP BY d.id
       ORDER BY d.created_at DESC`,
    ),
    ['id', 'owner'],
  );
}

export async function listEnabledDevicesForUser(db: D1Database, userId: string) {
  return allWithUuidFields<{
    id: string;
    owner: string;
    name: string;
    platform: string;
    enabled: number;
    created_at: number;
  }>(
    db
      .prepare(
        `SELECT id, owner, name, platform, enabled, created_at
         FROM devices
         WHERE owner = ? AND enabled = 1
         ORDER BY created_at DESC`,
      )
      .bind(uuidToBytes(userId)),
    ['id', 'owner'],
  );
}

export async function createBatch(
  db: D1Database,
  input: {
    id: string;
    user_id: string;
    device_id: string;
    url: string;
    start: number;
    end: number;
    end_hash: string;
    created_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO batches (id, user_id, device_id, url, start, end, end_hash, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(
      uuidToBytes(input.id),
      uuidToBytes(input.user_id),
      uuidToBytes(input.device_id),
      input.url,
      input.start,
      input.end,
      input.end_hash,
      input.created_at,
    )
    .run();
}

export async function createDeviceLog(
  db: D1Database,
  input: {
    id: string;
    user_id: string;
    device_id: string;
    ts: number;
    type: string;
    data: string;
    risk?: number | null;
    created_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO device_logs (id, user_id, device_id, ts, type, data, risk, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(
      uuidToBytes(input.id),
      uuidToBytes(input.user_id),
      uuidToBytes(input.device_id),
      input.ts,
      input.type,
      input.data,
      input.risk ?? null,
      input.created_at,
    )
    .run();
}

export async function listBatches(
  db: D1Database,
  ownerIds: string[],
  filters: { deviceId?: string; cursor?: number },
  limit: number,
) {
  if (ownerIds.length === 0) {
    return [];
  }

  const params: SqlValue[] = ownerIds.map(uuidToBytes);
  let query = `SELECT id, user_id, device_id, url, start, end, end_hash, created_at
               FROM batches
               WHERE user_id IN (${placeholders(ownerIds.length)})`;

  if (filters.deviceId) {
    query += ' AND device_id = ?';
    params.push(uuidToBytes(filters.deviceId));
  }

  if (filters.cursor !== undefined) {
    query += ' AND created_at < ?';
    params.push(filters.cursor);
  }

  query += ' ORDER BY created_at DESC LIMIT ?';
  params.push(limit);

  return allWithUuidFields<{
    id: string;
    user_id: string;
    device_id: string;
    url: string;
    start: number;
    end: number;
    end_hash: string;
    created_at: number;
  }>(db.prepare(query).bind(...params), ['id', 'user_id', 'device_id']);
}

export async function listBatchWindowsForUser(
  db: D1Database,
  userId: string,
  windowStart: number,
  windowEnd: number,
) {
  return allWithUuidFields<{ id: string; device_id: string; start: number; end: number }>(
    db
      .prepare(
        `SELECT id, device_id, start, end
         FROM batches
         WHERE user_id = ? AND end > ? AND start < ?
         ORDER BY start ASC`,
      )
      .bind(uuidToBytes(userId), windowStart, windowEnd),
    ['id', 'device_id'],
  );
}

export async function listDeviceLogs(
  db: D1Database,
  ownerIds: string[],
  filters: { deviceId?: string; cursor?: number },
  limit: number,
) {
  if (ownerIds.length === 0) {
    return [];
  }

  const params: SqlValue[] = ownerIds.map(uuidToBytes);
  let query = `SELECT id, user_id, device_id, ts, type, data, risk, created_at
               FROM device_logs
               WHERE user_id IN (${placeholders(ownerIds.length)})`;

  if (filters.deviceId) {
    query += ' AND device_id = ?';
    params.push(uuidToBytes(filters.deviceId));
  }

  if (filters.cursor !== undefined) {
    query += ' AND created_at < ?';
    params.push(filters.cursor);
  }

  query += ' ORDER BY created_at DESC LIMIT ?';
  params.push(limit);

  return allWithUuidFields<{
    id: string;
    user_id: string;
    device_id: string;
    ts: number;
    type: string;
    data: string;
    risk: number | null;
    created_at: number;
  }>(db.prepare(query).bind(...params), ['id', 'user_id', 'device_id']);
}

export async function findDeviceLogByKindWithinWindow(
  db: D1Database,
  input: { userId: string; deviceId: string; type: string; windowStart: number; windowEnd: number },
) {
  return firstWithUuidFields<{
    id: string;
    user_id: string;
    device_id: string;
    ts: number;
    type: string;
    data: string;
    risk: number | null;
    created_at: number;
  }>(
    db
      .prepare(
        `SELECT id, user_id, device_id, ts, type, data, risk, created_at
         FROM device_logs
         WHERE user_id = ? AND device_id = ? AND type = ? AND ts >= ? AND ts < ?
         ORDER BY ts DESC
         LIMIT 1`,
      )
      .bind(
        uuidToBytes(input.userId),
        uuidToBytes(input.deviceId),
        input.type,
        input.windowStart,
        input.windowEnd,
      ),
    ['id', 'user_id', 'device_id'],
  );
}

export async function listRiskDeviceLogsForUser(
  db: D1Database,
  userId: string,
  windowStart: number,
  windowEnd: number,
) {
  return allWithUuidFields<{
    id: string;
    user_id: string;
    device_id: string;
    ts: number;
    type: string;
    data: string;
    risk: number | null;
    created_at: number;
  }>(
    db
      .prepare(
        `SELECT id, user_id, device_id, ts, type, data, risk, created_at
         FROM device_logs
         WHERE user_id = ? AND risk IS NOT NULL AND ts >= ? AND ts < ?
         ORDER BY ts DESC`,
      )
      .bind(uuidToBytes(userId), windowStart, windowEnd),
    ['id', 'user_id', 'device_id'],
  );
}

export async function listDeviceLogsForUser(
  db: D1Database,
  userId: string,
  windowStart: number,
  windowEnd: number,
) {
  return allWithUuidFields<{
    id: string;
    user_id: string;
    device_id: string;
    ts: number;
    type: string;
    data: string;
    risk: number | null;
    created_at: number;
  }>(
    db
      .prepare(
        `SELECT id, user_id, device_id, ts, type, data, risk, created_at
         FROM device_logs
         WHERE user_id = ? AND ts >= ? AND ts < ?
         ORDER BY ts DESC`,
      )
      .bind(uuidToBytes(userId), windowStart, windowEnd),
    ['id', 'user_id', 'device_id'],
  );
}

export async function findPartnerInviteForOwner(db: D1Database, ownerId: string, email: string) {
  return firstWithUuidFields<{ id: string }>(
    db
      .prepare('SELECT id FROM partners WHERE user_id = ? AND partner_email = ?')
      .bind(uuidToBytes(ownerId), email),
    ['id'],
  );
}

export async function createPartner(
  db: D1Database,
  input: {
    id: string;
    user_id: string;
    partner_email: string;
    invite_token_hash: string;
    invite_expires_at: number;
    permissions: string;
    e2ee_key?: ArrayBuffer;
    created_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO partners (
        id, user_id, partner_user_id, partner_email, invite_token_hash, invite_expires_at,
        status, permissions, e2ee_key, created_at, updated_at
      ) VALUES (?, ?, NULL, ?, ?, ?, 'pending', ?, ?, ?, ?)`,
    )
    .bind(
      uuidToBytes(input.id),
      uuidToBytes(input.user_id),
      input.partner_email,
      input.invite_token_hash,
      input.invite_expires_at,
      input.permissions,
      input.e2ee_key ?? null,
      input.created_at,
      input.created_at,
    )
    .run();
}

export async function findPartnerById(db: D1Database, partnerId: string) {
  return firstWithUuidFields<{
    id: string;
    user_id: string;
    partner_user_id: string | null;
    partner_email: string;
    invite_token_hash: string | null;
    invite_expires_at: number | null;
    status: string;
    permissions: string;
    e2ee_key: ArrayBuffer | null;
    created_at: number;
    updated_at: number;
  }>(
    db
      .prepare(
        `SELECT id, user_id, partner_user_id, partner_email, invite_token_hash, invite_expires_at,
                status, permissions, e2ee_key, created_at, updated_at
         FROM partners WHERE id = ?`,
      )
      .bind(uuidToBytes(partnerId)),
    ['id', 'user_id', 'partner_user_id'],
  );
}

export async function findPartnerByInviteTokenHash(db: D1Database, tokenHash: string) {
  return firstWithUuidFields<{
    id: string;
    user_id: string;
    partner_user_id: string | null;
    partner_email: string;
    invite_token_hash: string | null;
    invite_expires_at: number | null;
    status: string;
    permissions: string;
    e2ee_key: ArrayBuffer | null;
    created_at: number;
    updated_at: number;
  }>(
    db
      .prepare(
        `SELECT id, user_id, partner_user_id, partner_email, invite_token_hash, invite_expires_at,
                status, permissions, e2ee_key, created_at, updated_at
         FROM partners
         WHERE invite_token_hash = ?`,
      )
      .bind(tokenHash),
    ['id', 'user_id', 'partner_user_id'],
  );
}

export async function findPartnerForOwnerAndUser(
  db: D1Database,
  ownerId: string,
  partnerUserId: string,
  excludeId?: string,
) {
  const query = excludeId
    ? 'SELECT id FROM partners WHERE user_id = ? AND partner_user_id = ? AND id != ?'
    : 'SELECT id FROM partners WHERE user_id = ? AND partner_user_id = ?';
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
  input: { id: string; partnerUserId: string; partnerEmail: string; updated_at: number },
) {
  return db
    .prepare(
      `UPDATE partners
       SET partner_user_id = ?, partner_email = ?, invite_token_hash = NULL, invite_expires_at = NULL,
           status = 'accepted', e2ee_key = NULL, updated_at = ?
       WHERE id = ?`,
    )
    .bind(
      uuidToBytes(input.partnerUserId),
      input.partnerEmail,
      input.updated_at,
      uuidToBytes(input.id),
    )
    .run();
}

export async function listOwnedPartners(db: D1Database, ownerId: string) {
  return allWithUuidFields<{
    id: string;
    status: string;
    permissions: string;
    created_at: number;
    e2ee_key: ArrayBuffer | null;
    partner_email: string;
    partner_id: string | null;
    partner_name: string | null;
  }>(
    db
      .prepare(
        `SELECT p.id, p.status, p.permissions, p.created_at, p.e2ee_key, p.partner_email,
                 u.id AS partner_id, u.name AS partner_name
           FROM partners p
           LEFT JOIN users u ON u.id = p.partner_user_id
           WHERE p.user_id = ?
           ORDER BY p.created_at DESC`,
      )
      .bind(uuidToBytes(ownerId)),
    ['id', 'partner_id'],
  );
}

export async function listIncomingPartners(db: D1Database, partnerUserId: string) {
  return allWithUuidFields<{
    id: string;
    status: string;
    permissions: string;
    created_at: number;
    e2ee_key: ArrayBuffer | null;
    owner_id: string;
    owner_email: string;
    owner_name: string | null;
  }>(
    db
      .prepare(
        `SELECT p.id, p.status, p.permissions, p.created_at, p.e2ee_key,
                u.id AS owner_id, u.email AS owner_email, u.name AS owner_name
          FROM partners p
          JOIN users u ON u.id = p.user_id
          WHERE p.partner_user_id = ?
          ORDER BY p.created_at DESC`,
      )
      .bind(uuidToBytes(partnerUserId)),
    ['id', 'owner_id'],
  );
}

export async function updatePartnerByOwner(
  db: D1Database,
  input: {
    id: string;
    ownerId: string;
    permissions?: string;
    e2ee_key?: ArrayBuffer;
    updated_at: number;
  },
) {
  const updates: string[] = ['updated_at = ?'];
  const params: (string | number | ArrayBuffer | null)[] = [input.updated_at];

  if (input.permissions !== undefined) {
    updates.push('permissions = ?');
    params.push(input.permissions);
  }

  if (input.e2ee_key !== undefined) {
    updates.push('e2ee_key = ?');
    params.push(input.e2ee_key);
  }

  params.push(uuidToBytes(input.id), uuidToBytes(input.ownerId));

  return db
    .prepare(`UPDATE partners SET ${updates.join(', ')} WHERE id = ? AND user_id = ?`)
    .bind(...params)
    .run();
}

export async function deletePartnerById(db: D1Database, partnerId: string) {
  return db.prepare('DELETE FROM partners WHERE id = ?').bind(uuidToBytes(partnerId)).run();
}

export async function upsertPartnerNotificationPreference(
  db: D1Database,
  input: {
    partnership_id: string;
    digest_cadence: string;
    immediate_tamper_severity: string;
    send_digest: boolean;
    updated_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO partner_notification_preferences (
         partnership_id, digest_cadence, immediate_tamper_severity, send_digest, created_at, updated_at
       ) VALUES (?, ?, ?, ?, ?, ?)
       ON CONFLICT(partnership_id) DO UPDATE SET
         digest_cadence = excluded.digest_cadence,
         immediate_tamper_severity = excluded.immediate_tamper_severity,
         send_digest = excluded.send_digest,
         updated_at = excluded.updated_at`,
    )
    .bind(
      uuidToBytes(input.partnership_id),
      input.digest_cadence,
      input.immediate_tamper_severity,
      input.send_digest ? 1 : 0,
      input.updated_at,
      input.updated_at,
    )
    .run();
}

export async function listNotificationPreferencesForPartner(db: D1Database, partnerUserId: string) {
  return allWithUuidFields<{
    partnership_id: string;
    status: string;
    owner_id: string;
    owner_email: string;
    owner_name: string | null;
    digest_cadence: string | null;
    immediate_tamper_severity: string | null;
    send_digest: number | null;
  }>(
    db
      .prepare(
        `SELECT p.id AS partnership_id,
                 p.status,
                 owner.id AS owner_id,
                 owner.email AS owner_email,
                 owner.name AS owner_name,
                 pref.digest_cadence,
                 pref.immediate_tamper_severity,
                 pref.send_digest
          FROM partners p
          JOIN users owner ON owner.id = p.user_id
          LEFT JOIN partner_notification_preferences pref ON pref.partnership_id = p.id
          WHERE p.partner_user_id = ?
          ORDER BY p.created_at DESC`,
      )
      .bind(uuidToBytes(partnerUserId)),
    ['partnership_id', 'owner_id'],
  );
}

export async function updatePartnerNotificationPreference(
  db: D1Database,
  input: {
    partnership_id: string;
    partner_user_id: string;
    digest_cadence?: string;
    immediate_tamper_severity?: string;
    send_digest?: boolean;
    updated_at: number;
  },
) {
  const partnership = await db
    .prepare('SELECT id FROM partners WHERE id = ? AND partner_user_id = ?')
    .bind(uuidToBytes(input.partnership_id), uuidToBytes(input.partner_user_id))
    .first<{ id: string }>();

  if (!partnership) {
    return null;
  }

  const current = await db
    .prepare(
      'SELECT digest_cadence, immediate_tamper_severity, send_digest FROM partner_notification_preferences WHERE partnership_id = ?',
    )
    .bind(uuidToBytes(input.partnership_id))
    .first<{
      digest_cadence: string;
      immediate_tamper_severity: string;
      send_digest: number;
    }>();

  const digestCadence = input.digest_cadence ?? current?.digest_cadence ?? 'daily';
  const severity =
    input.immediate_tamper_severity ?? current?.immediate_tamper_severity ?? 'critical';
  const sendDigest = input.send_digest ?? (current ? current.send_digest === 1 : true);

  await upsertPartnerNotificationPreference(db, {
    partnership_id: input.partnership_id,
    digest_cadence: digestCadence,
    immediate_tamper_severity: severity,
    send_digest: sendDigest,
    updated_at: input.updated_at,
  });

  return {
    partnership_id: input.partnership_id,
    digest_cadence: digestCadence,
    immediate_tamper_severity: severity,
    send_digest: sendDigest,
  };
}

export async function clearPartnerAccessKeysForUser(db: D1Database, partnerUserId: string) {
  return db
    .prepare('UPDATE partners SET e2ee_key = NULL WHERE partner_user_id = ?')
    .bind(uuidToBytes(partnerUserId))
    .run();
}

export async function listPartnerAccessTargetsForOwner(db: D1Database, ownerId: string) {
  const result = await allWithUuidFields<{
    id: string;
    permissions: string;
    partner_user_id: string | null;
    partner_email: string | null;
    partner_pub_key: ArrayBuffer | null;
  }>(
    db
      .prepare(
        `SELECT p.id,
                p.permissions,
                p.partner_user_id,
                recipient.email AS partner_email,
                recipient.pub_key AS partner_pub_key
         FROM partners p
         LEFT JOIN users recipient ON recipient.id = p.partner_user_id
         WHERE p.user_id = ? AND p.status = 'accepted'`,
      )
      .bind(uuidToBytes(ownerId)),
    ['id', 'partner_user_id'],
  );

  return result.filter(
    (row) =>
      parsePermissions(row.permissions).view_data && row.partner_user_id && row.partner_email,
  );
}

export async function updatePartnerAccessKeys(
  db: D1Database,
  ownerId: string,
  keys: Array<{ partnership_id: string; e2ee_key: ArrayBuffer }>,
) {
  for (const key of keys) {
    await db
      .prepare('UPDATE partners SET e2ee_key = ?, updated_at = ? WHERE id = ? AND user_id = ?')
      .bind(key.e2ee_key, Date.now(), uuidToBytes(key.partnership_id), uuidToBytes(ownerId))
      .run();
  }
}

export async function listAcceptedNotificationTargetsForUser(db: D1Database, userId: string) {
  return allWithUuidFields<{
    partnership_id: string;
    partner_email: string;
    partner_user_id: string | null;
    partner_name: string | null;
    digest_cadence: string | null;
    immediate_tamper_severity: string | null;
    send_digest: number | null;
  }>(
    db
      .prepare(
        `SELECT p.id AS partnership_id,
                recipient.email AS partner_email,
                recipient.id AS partner_user_id,
                recipient.name AS partner_name,
                pref.digest_cadence,
                pref.immediate_tamper_severity,
                pref.send_digest
          FROM partners p
          JOIN users recipient ON recipient.id = p.partner_user_id
          LEFT JOIN partner_notification_preferences pref ON pref.partnership_id = p.id
          WHERE p.user_id = ? AND p.status = 'accepted'`,
      )
      .bind(uuidToBytes(userId)),
    ['partnership_id', 'partner_user_id'],
  );
}

export async function createEmailToken(
  db: D1Database,
  input: {
    id: string;
    user_id: string;
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
      uuidToBytes(input.user_id),
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
    user_id: string;
    email: string;
    purpose: string;
    token_hash: string;
    expires_at: number;
    consumed_at: number | null;
    created_at: number;
  }>(purpose ? prepared.bind(tokenHash, purpose) : prepared.bind(tokenHash), ['id', 'user_id']);
}

export async function consumeEmailToken(db: D1Database, tokenId: string, consumedAt: number) {
  return db
    .prepare('UPDATE email_tokens SET consumed_at = ? WHERE id = ? AND consumed_at IS NULL')
    .bind(consumedAt, uuidToBytes(tokenId))
    .run();
}

export async function createSessionRecord(
  db: D1Database,
  input: {
    id: string;
    session_type: SessionType;
    user_id?: string;
    device_id?: string;
    refresh_token_hash: string;
    expires_at: number;
    created_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO sessions (
         id, session_type, user_id, device_id, refresh_token_hash, expires_at, created_at
       ) VALUES (?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(
      input.id,
      input.session_type,
      input.user_id ? uuidToBytes(input.user_id) : null,
      input.device_id ? uuidToBytes(input.device_id) : null,
      input.refresh_token_hash,
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
  return firstWithUuidFields<{
    id: string;
    session_type: SessionType;
    user_id: string | null;
    device_id: string | null;
    refresh_token_hash: string;
    expires_at: number;
    created_at: number;
  }>(
    db
      .prepare(
        `SELECT id, session_type, user_id, device_id, refresh_token_hash, expires_at, created_at
         FROM sessions
         WHERE refresh_token_hash = ? AND session_type = ?`,
      )
      .bind(refreshTokenHash, sessionType),
    ['user_id', 'device_id'],
  );
}

export async function deleteSessionByRefreshTokenHash(
  db: D1Database,
  refreshTokenHash: string,
  sessionType?: SessionType,
) {
  return sessionType
    ? db
        .prepare('DELETE FROM sessions WHERE refresh_token_hash = ? AND session_type = ?')
        .bind(refreshTokenHash, sessionType)
        .run()
    : db.prepare('DELETE FROM sessions WHERE refresh_token_hash = ?').bind(refreshTokenHash).run();
}

export async function listDigestEligiblePartnerships(db: D1Database) {
  return allWithUuidFields<{
    partnership_id: string;
    user_id: string;
    partner_email: string;
    owner_email: string;
    owner_name: string | null;
    digest_cadence: string | null;
    immediate_tamper_severity: string | null;
    send_digest: number | null;
  }>(
    db.prepare(
      `SELECT p.id AS partnership_id,
              p.user_id,
              recipient.email AS partner_email,
              owner.email AS owner_email,
              owner.name AS owner_name,
              pref.digest_cadence,
              pref.immediate_tamper_severity,
              pref.send_digest
        FROM partners p
        JOIN users owner ON owner.id = p.user_id
        JOIN users recipient ON recipient.id = p.partner_user_id
        LEFT JOIN partner_notification_preferences pref ON pref.partnership_id = p.id
        WHERE p.status = 'accepted'`,
    ),
    ['partnership_id', 'user_id'],
  );
}

export async function getHashState(db: D1Database, deviceId: string) {
  return firstWithUuidFields<{
    device_id: string;
    user_id: string;
    state: ArrayBuffer;
    updated_at: number;
  }>(
    db
      .prepare('SELECT device_id, user_id, state, updated_at FROM hash_states WHERE device_id = ?')
      .bind(uuidToBytes(deviceId)),
    ['device_id', 'user_id'],
  );
}

export async function upsertHashState(
  db: D1Database,
  input: { device_id: string; user_id: string; state: ArrayBuffer; updated_at: number },
) {
  return db
    .prepare(
      `INSERT INTO hash_states (device_id, user_id, state, updated_at)
       VALUES (?, ?, ?, ?)
       ON CONFLICT(device_id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at`,
    )
    .bind(uuidToBytes(input.device_id), uuidToBytes(input.user_id), input.state, input.updated_at)
    .run();
}

export async function resetHashState(
  db: D1Database,
  deviceId: string,
  userId: string,
  updatedAt: number,
) {
  return upsertHashState(db, {
    device_id: deviceId,
    user_id: userId,
    state: new Uint8Array(32).buffer,
    updated_at: updatedAt,
  });
}
