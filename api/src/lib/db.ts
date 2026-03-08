type SqlValue = string | number | null;

type PartnerPermissions = {
  view_data?: boolean;
};

function placeholders(count: number) {
  return Array.from({ length: count }, () => '?').join(', ');
}

function parsePermissions(value: string): PartnerPermissions {
  return JSON.parse(value) as PartnerPermissions;
}

export async function findUserByEmail(db: D1Database, email: string) {
  return db
    .prepare(
      'SELECT id, email, password_hash, name, e2ee_key, pub_key, priv_key FROM users WHERE email = ?',
    )
    .bind(email)
    .first<{
      id: string;
      email: string;
      password_hash: string;
      name: string | null;
      e2ee_key: ArrayBuffer | null;
      pub_key: ArrayBuffer | null;
      priv_key: ArrayBuffer | null;
    }>();
}

export async function findUserById(db: D1Database, userId: string) {
  return db
    .prepare('SELECT id, email, name, e2ee_key, pub_key, priv_key FROM users WHERE id = ?')
    .bind(userId)
    .first<{
      id: string;
      email: string;
      name: string | null;
      e2ee_key: ArrayBuffer | null;
      pub_key: ArrayBuffer | null;
      priv_key: ArrayBuffer | null;
    }>();
}

export async function findUserPublicKeyByEmail(db: D1Database, email: string) {
  return db
    .prepare('SELECT id, pub_key FROM users WHERE email = ?')
    .bind(email)
    .first<{ id: string; pub_key: ArrayBuffer | null }>();
}

export async function createUser(
  db: D1Database,
  input: { id: string; email: string; passwordHash: string; name?: string },
) {
  return db
    .prepare(
      'INSERT INTO users (id, email, password_hash, name, created_at) VALUES (?, ?, ?, ?, ?)',
    )
    .bind(input.id, input.email, input.passwordHash, input.name ?? null, Date.now())
    .run();
}

export async function updateUser(
  db: D1Database,
  userId: string,
  fields: { name?: string; e2ee_key?: ArrayBuffer; pub_key?: ArrayBuffer; priv_key?: ArrayBuffer },
) {
  const updates: string[] = [];
  const params: (string | ArrayBuffer | null)[] = [];

  if (fields.name !== undefined) {
    updates.push('name = ?');
    params.push(fields.name);
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

  params.push(userId);
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
    .bind(input.id, input.owner, input.name, input.platform, Date.now())
    .run();
}

export async function findDeviceById(db: D1Database, deviceId: string) {
  return db
    .prepare('SELECT id, owner, name, platform, enabled, created_at FROM devices WHERE id = ?')
    .bind(deviceId)
    .first<{
      id: string;
      owner: string;
      name: string;
      platform: string;
      enabled: number;
      created_at: number;
    }>();
}

export async function findOwnedDevice(db: D1Database, deviceId: string, ownerId: string) {
  return db
    .prepare(
      'SELECT id, owner, name, platform, enabled, created_at FROM devices WHERE id = ? AND owner = ?',
    )
    .bind(deviceId, ownerId)
    .first<{
      id: string;
      owner: string;
      name: string;
      platform: string;
      enabled: number;
      created_at: number;
    }>();
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

  params.push(deviceId);
  return db
    .prepare(`UPDATE devices SET ${updates.join(', ')} WHERE id = ?`)
    .bind(...params)
    .run();
}

export async function listVisibleOwnerIds(db: D1Database, requesterId: string) {
  const partnerships = await db
    .prepare(
      "SELECT user_id, permissions FROM partners WHERE partner_user_id = ? AND status = 'accepted'",
    )
    .bind(requesterId)
    .all<{ user_id: string; permissions: string }>();

  return [
    requesterId,
    ...partnerships.results
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
    .bind(ownerId, requesterId)
    .first<{ permissions: string }>();

  return partnership ? Boolean(parsePermissions(partnership.permissions).view_data) : false;
}

export async function listDevicesForOwners(db: D1Database, ownerIds: string[]) {
  if (ownerIds.length === 0) {
    return [];
  }

  const result = await db
    .prepare(
      `SELECT d.id, d.owner, d.name, d.platform, d.enabled, d.created_at, MAX(b.end) AS last_upload_at
       FROM devices d
       LEFT JOIN batches b ON b.device_id = d.id
       WHERE d.owner IN (${placeholders(ownerIds.length)})
       GROUP BY d.id
       ORDER BY d.created_at DESC`,
    )
    .bind(...ownerIds)
    .all<{
      id: string;
      owner: string;
      name: string;
      platform: string;
      enabled: number;
      created_at: number;
      last_upload_at: number | null;
    }>();

  return result.results;
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
      input.id,
      input.user_id,
      input.device_id,
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
    created_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO device_logs (id, user_id, device_id, ts, type, data, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
    )
    .bind(
      input.id,
      input.user_id,
      input.device_id,
      input.ts,
      input.type,
      input.data,
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

  const params: SqlValue[] = [...ownerIds];
  let query = `SELECT id, user_id, device_id, url, start, end, end_hash, created_at
               FROM batches
               WHERE user_id IN (${placeholders(ownerIds.length)})`;

  if (filters.deviceId) {
    query += ' AND device_id = ?';
    params.push(filters.deviceId);
  }

  if (filters.cursor !== undefined) {
    query += ' AND created_at < ?';
    params.push(filters.cursor);
  }

  query += ' ORDER BY created_at DESC LIMIT ?';
  params.push(limit);

  const result = await db
    .prepare(query)
    .bind(...params)
    .all<{
      id: string;
      user_id: string;
      device_id: string;
      url: string;
      start: number;
      end: number;
      end_hash: string;
      created_at: number;
    }>();

  return result.results;
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

  const params: SqlValue[] = [...ownerIds];
  let query = `SELECT id, user_id, device_id, ts, type, data, created_at
               FROM device_logs
               WHERE user_id IN (${placeholders(ownerIds.length)})`;

  if (filters.deviceId) {
    query += ' AND device_id = ?';
    params.push(filters.deviceId);
  }

  if (filters.cursor !== undefined) {
    query += ' AND created_at < ?';
    params.push(filters.cursor);
  }

  query += ' ORDER BY created_at DESC LIMIT ?';
  params.push(limit);

  const result = await db
    .prepare(query)
    .bind(...params)
    .all<{
      id: string;
      user_id: string;
      device_id: string;
      ts: number;
      type: string;
      data: string;
      created_at: number;
    }>();

  return result.results;
}

export async function findPartnerInviteForOwner(
  db: D1Database,
  ownerId: string,
  email: string,
  partnerUserId?: string,
) {
  const queries = [
    db
      .prepare('SELECT id FROM partners WHERE user_id = ? AND partner_email = ?')
      .bind(ownerId, email)
      .first<{ id: string }>(),
  ];

  if (partnerUserId) {
    queries.push(
      db
        .prepare('SELECT id FROM partners WHERE user_id = ? AND partner_user_id = ?')
        .bind(ownerId, partnerUserId)
        .first<{ id: string }>(),
    );
  }

  const results = await Promise.all(queries);
  return results.find(Boolean) ?? null;
}

export async function createPartner(
  db: D1Database,
  input: {
    id: string;
    user_id: string;
    partner_user_id?: string;
    partner_email: string;
    permissions: string;
    e2ee_key?: ArrayBuffer;
    created_at: number;
  },
) {
  return db
    .prepare(
      `INSERT INTO partners (
        id, user_id, partner_user_id, partner_email, status, permissions, e2ee_key, created_at, updated_at
      ) VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?)`,
    )
    .bind(
      input.id,
      input.user_id,
      input.partner_user_id ?? null,
      input.partner_email,
      input.permissions,
      input.e2ee_key ?? null,
      input.created_at,
      input.created_at,
    )
    .run();
}

export async function findPartnerById(db: D1Database, partnerId: string) {
  return db
    .prepare(
      `SELECT id, user_id, partner_user_id, partner_email, status, permissions, e2ee_key, created_at, updated_at
       FROM partners WHERE id = ?`,
    )
    .bind(partnerId)
    .first<{
      id: string;
      user_id: string;
      partner_user_id: string | null;
      partner_email: string;
      status: string;
      permissions: string;
      e2ee_key: ArrayBuffer | null;
      created_at: number;
      updated_at: number;
    }>();
}

export async function acceptPartner(
  db: D1Database,
  input: { id: string; partnerUserId: string; updated_at: number },
) {
  return db
    .prepare(
      `UPDATE partners
       SET partner_user_id = ?, status = 'accepted', updated_at = ?
       WHERE id = ?`,
    )
    .bind(input.partnerUserId, input.updated_at, input.id)
    .run();
}

export async function listOwnedPartners(db: D1Database, ownerId: string) {
  const result = await db
    .prepare(
      `SELECT p.id, p.status, p.permissions, p.created_at, p.e2ee_key, p.partner_email,
              u.id AS partner_id, u.name AS partner_name
        FROM partners p
        LEFT JOIN users u ON u.id = p.partner_user_id OR (p.partner_user_id IS NULL AND u.email = p.partner_email)
        WHERE p.user_id = ?
        ORDER BY p.created_at DESC`,
    )
    .bind(ownerId)
    .all<{
      id: string;
      status: string;
      permissions: string;
      created_at: number;
      e2ee_key: ArrayBuffer | null;
      partner_email: string;
      partner_id: string | null;
      partner_name: string | null;
    }>();

  return result.results;
}

export async function listIncomingPartners(
  db: D1Database,
  partnerUserId: string,
  partnerEmail: string,
) {
  const result = await db
    .prepare(
      `SELECT p.id, p.status, p.permissions, p.created_at, p.e2ee_key,
              u.id AS owner_id, u.email AS owner_email, u.name AS owner_name
        FROM partners p
        JOIN users u ON u.id = p.user_id
        WHERE p.partner_user_id = ? OR (p.partner_user_id IS NULL AND p.partner_email = ?)
        ORDER BY p.created_at DESC`,
    )
    .bind(partnerUserId, partnerEmail)
    .all<{
      id: string;
      status: string;
      permissions: string;
      created_at: number;
      e2ee_key: ArrayBuffer | null;
      owner_id: string;
      owner_email: string;
      owner_name: string | null;
    }>();

  return result.results;
}

export async function updatePartnerByOwner(
  db: D1Database,
  input: {
    id: string;
    ownerId: string;
    partner_user_id?: string;
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

  if (input.partner_user_id !== undefined) {
    updates.push('partner_user_id = ?');
    params.push(input.partner_user_id);
  }

  if (input.e2ee_key !== undefined) {
    updates.push('e2ee_key = ?');
    params.push(input.e2ee_key);
  }

  params.push(input.id, input.ownerId);

  return db
    .prepare(`UPDATE partners SET ${updates.join(', ')} WHERE id = ? AND user_id = ?`)
    .bind(...params)
    .run();
}

export async function deletePartnerById(db: D1Database, partnerId: string) {
  return db.prepare('DELETE FROM partners WHERE id = ?').bind(partnerId).run();
}

export async function getHashState(db: D1Database, deviceId: string) {
  return db
    .prepare('SELECT device_id, user_id, state, updated_at FROM hash_states WHERE device_id = ?')
    .bind(deviceId)
    .first<{
      device_id: string;
      user_id: string;
      state: ArrayBuffer;
      updated_at: number;
    }>();
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
    .bind(input.device_id, input.user_id, input.state, input.updated_at)
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
