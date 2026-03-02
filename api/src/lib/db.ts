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
  createdAt: string,
) {
  return db
    .prepare(
      `INSERT INTO devices (id, user_id, name, platform, enabled, created_at)
     VALUES (?, ?, ?, ?, ?, ?)`,
    )
    .bind(id, userId, name, platform, 1, createdAt)
    .run();
}

export async function listDevices(db: D1Database, userId: string) {
  return db
    .prepare(
      `SELECT d.id, d.name, d.platform,
       (SELECT updated_at FROM device_states WHERE user_id = ? AND device_id = d.id) AS last_seen_at,
       (SELECT created_at FROM r2_batches WHERE user_id = ? AND device_id = d.id ORDER BY created_at DESC LIMIT 1) AS last_upload_at,
       d.enabled
       FROM devices d WHERE d.user_id = ? ORDER BY d.created_at DESC`,
    )
    .bind(userId, userId, userId)
    .all<{
      id: string;
      name: string;
      platform: string;
      last_seen_at: string | null;
      last_upload_at: string | null;
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
  fields: { name?: string; enabled?: boolean },
) {
  const updates: string[] = [];
  const params: (string | number)[] = [];

  if (fields.name !== undefined) {
    updates.push('name = ?');
    params.push(fields.name);
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



export async function getDeviceState(
  db: D1Database,
  deviceId: string,
): Promise<{ state: ArrayBuffer; batch_start_state: ArrayBuffer | null } | null> {
  return db
    .prepare('SELECT state, batch_start_state FROM device_states WHERE device_id = ?')
    .bind(deviceId)
    .first<{ state: ArrayBuffer; batch_start_state: ArrayBuffer | null }>();
}

export async function upsertDeviceState(
  db: D1Database,
  deviceId: string,
  userId: string,
  state: ArrayBuffer,
  updatedAt: string,
  batchStartState?: ArrayBuffer,
) {
  if (batchStartState !== undefined) {
    return db
      .prepare(
        `INSERT INTO device_states (device_id, user_id, state, batch_start_state, updated_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(device_id) DO UPDATE SET state = excluded.state, batch_start_state = excluded.batch_start_state, updated_at = excluded.updated_at`,
      )
      .bind(deviceId, userId, state, batchStartState, updatedAt)
      .run();
  }
  return db
    .prepare(
      `INSERT INTO device_states (device_id, user_id, state, updated_at) VALUES (?, ?, ?, ?)
       ON CONFLICT(device_id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at`,
    )
    .bind(deviceId, userId, state, updatedAt)
    .run();
}

export async function createBatch(
  db: D1Database,
  id: string,
  userId: string,
  deviceId: string,
  r2Key: string,
  startTime: string,
  endTime: string,
  startChainHash: string,
  endChainHash: string,
  itemCount: number,
  sizeBytes: number,
  createdAt: string,
) {
  return db
    .prepare(
      `INSERT INTO r2_batches
         (id, user_id, device_id, r2_key, start_time, end_time,
          start_chain_hash, end_chain_hash, item_count, size_bytes, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(
      id, userId, deviceId, r2Key, startTime, endTime,
      startChainHash, endChainHash, itemCount, sizeBytes, createdAt,
    )
    .run();
}

export async function listBatches(
  db: D1Database,
  userId: string,
  filters: { device_id?: string; cursor?: string },
  limit: number,
) {
  let query =
    `SELECT id, device_id, r2_key, start_time, end_time,
            start_chain_hash, end_chain_hash, item_count, size_bytes, created_at
     FROM r2_batches WHERE user_id = ?`;
  const params: (string | number)[] = [userId];

  if (filters.device_id) {
    query += ' AND device_id = ?';
    params.push(filters.device_id);
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
      device_id: string;
      r2_key: string;
      start_time: string;
      end_time: string;
      start_chain_hash: string;
      end_chain_hash: string;
      item_count: number;
      size_bytes: number;
      created_at: string;
    }>();

  return {
    items: result.results.slice(0, limit),
    hasMore: result.results.length > limit,
  };
}

export async function findBatchById(
  db: D1Database,
  batchId: string,
) {
  return db
    .prepare(
      `SELECT id, user_id, device_id, r2_key, start_time, end_time,
              start_chain_hash, end_chain_hash, item_count, size_bytes, created_at
       FROM r2_batches WHERE id = ?`,
    )
    .bind(batchId)
    .first<{
      id: string;
      user_id: string;
      device_id: string;
      r2_key: string;
      start_time: string;
      end_time: string;
      start_chain_hash: string;
      end_chain_hash: string;
      item_count: number;
      size_bytes: number;
      created_at: string;
    }>();
}

// ── Chain hashes ──────────────────────────────────────────────────────────────

export async function createChainHash(
  db: D1Database,
  id: string,
  userId: string,
  deviceId: string,
  hash: ArrayBuffer,
  clientTimestamp: string,
  createdAt: string,
) {
  return db
    .prepare(
      `INSERT INTO chain_hashes (id, user_id, device_id, hash, client_timestamp, created_at)
       VALUES (?, ?, ?, ?, ?, ?)`,
    )
    .bind(id, userId, deviceId, hash, clientTimestamp, createdAt)
    .run();
}

/** Returns ISO string of the most recent hash for this device, or null. */
export async function getLastChainHashTimestamp(
  db: D1Database,
  userId: string,
  deviceId: string,
): Promise<string | null> {
  const row = await db
    .prepare(
      'SELECT created_at FROM chain_hashes WHERE user_id = ? AND device_id = ? ORDER BY created_at DESC LIMIT 1',
    )
    .bind(userId, deviceId)
    .first<{ created_at: string }>();
  return row?.created_at ?? null;
}

export async function queryChainHashes(
  db: D1Database,
  userId: string,
  deviceId: string,
  from: string,
  to: string,
  cursor: string | undefined,
  limit: number,
) {
  let query =
    `SELECT id, hash, client_timestamp, created_at
     FROM chain_hashes
     WHERE user_id = ? AND device_id = ?
       AND client_timestamp >= ? AND client_timestamp <= ?`;
  const params: (string | number)[] = [userId, deviceId, from, to];

  if (cursor) {
    query += ' AND client_timestamp > ?';
    params.push(cursor);
  }

  query += ' ORDER BY client_timestamp ASC LIMIT ?';
  params.push(limit + 1);

  const result = await db
    .prepare(query)
    .bind(...params)
    .all<{
      id: string;
      hash: ArrayBuffer;
      client_timestamp: string;
      created_at: string;
    }>();

  return {
    items: result.results.slice(0, limit),
    hasMore: result.results.length > limit,
  };
}

// ── Partners ──────────────────────────────────────────────────────────────────

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
        `SELECT p.id, p.partner_user_id as partner_user_id, u.email as partner_email, p.status, p.permissions, p.created_at
       FROM partners p JOIN users u ON p.partner_user_id = u.id
       WHERE p.user_id = ?`,
      )
      .bind(userId)
      .all<{
        id: string;
        partner_user_id: string;
        partner_email: string;
        status: string;
        permissions: string;
        created_at: string;
      }>(),
    db
      .prepare(
        `SELECT p.id, p.user_id as partner_user_id, u.email as partner_email, p.status, p.permissions, p.created_at
       FROM partners p JOIN users u ON p.user_id = u.id
       WHERE p.partner_user_id = ?`,
      )
      .bind(userId)
      .all<{
        id: string;
        partner_user_id: string;
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

export async function findPartnerByEitherParty(db: D1Database, partnerId: string, userId: string) {
  return db
    .prepare(`
      SELECT p.id, u1.email as owner_email, u2.email as partner_email
      FROM partners p
      JOIN users u1 ON p.user_id = u1.id
      JOIN users u2 ON p.partner_user_id = u2.id
      WHERE p.id = ? AND (p.user_id = ? OR p.partner_user_id = ?)
    `)
    .bind(partnerId, userId, userId)
    .first<{ id: string; owner_email: string; partner_email: string }>();
}

export async function findAcceptedPartnership(
  db: D1Database,
  ownerId: string,
  requesterId: string,
) {
  return db
    .prepare(
      `SELECT permissions FROM partners
       WHERE user_id = ? AND partner_user_id = ? AND status = 'accepted'`,
    )
    .bind(ownerId, requesterId)
    .first<{ permissions: string }>();
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
