// Database query helpers — all raw D1 SQL lives here

export async function findUserByEmail(db: D1Database, email: string) {
  return db
    .prepare('SELECT id, email, password_hash FROM users WHERE email = ?')
    .bind(email)
    .first<{
      id: string;
      email: string;
      password_hash: string;
    }>();
}

export async function createUser(
  db: D1Database,
  id: string,
  email: string,
  passwordHash: string,
  name: string | null,
  createdAt: string,
) {
  return db
    .prepare(
      'INSERT INTO users (id, email, password_hash, name, created_at) VALUES (?, ?, ?, ?, ?)',
    )
    .bind(id, email, passwordHash, name, createdAt)
    .run();
}

export async function createDevice(
  db: D1Database,
  id: string,
  userId: string,
  name: string,
  platform: string,
  avgIntervalSeconds: number,
  createdAt: string,
) {
  return db
    .prepare(
      `INSERT INTO devices (id, user_id, name, platform, avg_interval_seconds, enabled, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(id, userId, name, platform, avgIntervalSeconds, 1, createdAt)
    .run();
}

export async function listDevices(db: D1Database, userId: string) {
  return db
    .prepare(
      `SELECT id, name, platform, last_seen_at, last_upload_at, avg_interval_seconds, enabled
     FROM devices WHERE user_id = ? ORDER BY created_at DESC`,
    )
    .bind(userId)
    .all<{
      id: string;
      name: string;
      platform: string;
      last_seen_at: string | null;
      last_upload_at: string | null;
      avg_interval_seconds: number;
      enabled: number;
    }>();
}

export async function findDevice(db: D1Database, deviceId: string, userId: string) {
  return db
    .prepare('SELECT id FROM devices WHERE id = ? AND user_id = ?')
    .bind(deviceId, userId)
    .first<{ id: string }>();
}

export async function updateDevice(
  db: D1Database,
  deviceId: string,
  fields: { name?: string; interval_seconds?: number; enabled?: boolean },
) {
  const updates: string[] = [];
  const params: (string | number)[] = [];

  if (fields.name !== undefined) {
    updates.push('name = ?');
    params.push(fields.name);
  }
  if (fields.interval_seconds !== undefined) {
    updates.push('avg_interval_seconds = ?');
    params.push(fields.interval_seconds);
  }
  if (fields.enabled !== undefined) {
    updates.push('enabled = ?');
    params.push(fields.enabled ? 1 : 0);
  }

  params.push(deviceId);
  return db
    .prepare(`UPDATE devices SET ${updates.join(', ')} WHERE id = ?`)
    .bind(...params)
    .run();
}

export async function updateDeviceActivity(db: D1Database, deviceId: string, timestamp: string) {
  return db
    .prepare('UPDATE devices SET last_seen_at = ?, last_upload_at = ? WHERE id = ?')
    .bind(timestamp, timestamp, deviceId)
    .run();
}

export async function createImage(
  db: D1Database,
  id: string,
  userId: string,
  deviceId: string,
  r2Key: string,
  sha256: string,
  contentType: string,
  sizeBytes: number,
  takenAt: string,
  createdAt: string,
) {
  return db
    .prepare(
      `INSERT INTO images (id, user_id, device_id, r2_key, sha256, content_type, size_bytes, status, taken_at, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, 'pending_upload', ?, ?)`,
    )
    .bind(id, userId, deviceId, r2Key, sha256, contentType, sizeBytes, takenAt, createdAt)
    .run();
}

export async function findImageById(db: D1Database, imageId: string) {
  return db
    .prepare('SELECT r2_key FROM images WHERE id = ?')
    .bind(imageId)
    .first<{ r2_key: string }>();
}

export async function createLog(
  db: D1Database,
  id: string,
  userId: string,
  deviceId: string,
  imageId: string | null,
  type: string,
  metadata: string | null,
  createdAt: string,
) {
  return db
    .prepare(
      `INSERT INTO logs (id, user_id, device_id, image_id, type, metadata, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(id, userId, deviceId, imageId, type, metadata, createdAt)
    .run();
}

export async function queryLogs(
  db: D1Database,
  userId: string,
  filters: { device_id?: string; type?: string; cursor?: string },
  limit: number,
) {
  let query =
    'SELECT id, type, device_id, image_id, metadata, created_at FROM logs WHERE user_id = ?';
  const params: (string | number)[] = [userId];

  if (filters.device_id) {
    query += ' AND device_id = ?';
    params.push(filters.device_id);
  }
  if (filters.type) {
    query += ' AND type = ?';
    params.push(filters.type);
  }
  if (filters.cursor) {
    query += ' AND created_at < ?';
    params.push(filters.cursor);
  }

  query += ' ORDER BY created_at DESC LIMIT ?';
  params.push(limit + 1);

  const result = await db
    .prepare(query)
    .bind(...params)
    .all<{
      id: string;
      type: string;
      device_id: string;
      image_id: string | null;
      metadata: string | null;
      created_at: string;
    }>();

  return {
    items: result.results.slice(0, limit),
    hasMore: result.results.length > limit,
  };
}

export async function findUserById(db: D1Database, userId: string) {
  return db.prepare('SELECT id FROM users WHERE id = ?').bind(userId).first<{ id: string }>();
}

export async function createPartner(
  db: D1Database,
  id: string,
  userId: string,
  partnerUserId: string,
  permissions: string,
  createdAt: string,
) {
  return db
    .prepare(
      `INSERT INTO partners (id, user_id, partner_user_id, status, permissions, created_at, updated_at)
     VALUES (?, ?, ?, 'pending', ?, ?, ?)`,
    )
    .bind(id, userId, partnerUserId, permissions, createdAt, createdAt)
    .run();
}

export async function findPartnerByUsers(db: D1Database, userId: string, partnerUserId: string) {
  return db
    .prepare('SELECT id FROM partners WHERE user_id = ? AND partner_user_id = ?')
    .bind(userId, partnerUserId)
    .first<{ id: string }>();
}

export async function findPartnerInvite(db: D1Database, partnerId: string, userId: string) {
  return db
    .prepare(`SELECT id FROM partners WHERE id = ? AND partner_user_id = ? AND status = 'pending'`)
    .bind(partnerId, userId)
    .first<{ id: string }>();
}

export async function acceptPartner(db: D1Database, id: string, updatedAt: string) {
  return db
    .prepare(`UPDATE partners SET status = 'accepted', updated_at = ? WHERE id = ?`)
    .bind(updatedAt, id)
    .run();
}

export async function listPartners(db: D1Database, userId: string) {
  const [owned, asPartner] = await Promise.all([
    db
      .prepare(
        `SELECT p.id, u.email as partner_email, p.status, p.permissions, p.created_at
       FROM partners p JOIN users u ON p.partner_user_id = u.id
       WHERE p.user_id = ?`,
      )
      .bind(userId)
      .all<{
        id: string;
        partner_email: string;
        status: string;
        permissions: string;
        created_at: string;
      }>(),
    db
      .prepare(
        `SELECT p.id, u.email as partner_email, p.status, p.permissions, p.created_at
       FROM partners p JOIN users u ON p.user_id = u.id
       WHERE p.partner_user_id = ?`,
      )
      .bind(userId)
      .all<{
        id: string;
        partner_email: string;
        status: string;
        permissions: string;
        created_at: string;
      }>(),
  ]);
  return { owned: owned.results, asPartner: asPartner.results };
}

export async function findPartnerByOwner(db: D1Database, partnerId: string, userId: string) {
  return db
    .prepare('SELECT id, permissions FROM partners WHERE id = ? AND user_id = ?')
    .bind(partnerId, userId)
    .first<{ id: string; permissions: string }>();
}

export async function updatePartnerPermissions(
  db: D1Database,
  partnerId: string,
  permissions: string,
  updatedAt: string,
) {
  return db
    .prepare('UPDATE partners SET permissions = ?, updated_at = ? WHERE id = ?')
    .bind(permissions, updatedAt, partnerId)
    .run();
}

export async function deletePartner(db: D1Database, partnerId: string) {
  return db.prepare('DELETE FROM partners WHERE id = ?').bind(partnerId).run();
}

export async function getSettings(db: D1Database, userId: string) {
  return db
    .prepare('SELECT data FROM settings WHERE user_id = ?')
    .bind(userId)
    .first<{ data: string }>();
}

export async function saveSettings(
  db: D1Database,
  userId: string,
  data: string,
  updatedAt: string,
) {
  return db
    .prepare(
      `INSERT INTO settings (user_id, data, updated_at) VALUES (?, ?, ?)
     ON CONFLICT(user_id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at`,
    )
    .bind(userId, data, updatedAt)
    .run();
}
