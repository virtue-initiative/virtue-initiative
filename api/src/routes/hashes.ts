import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import z from 'zod';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { listHashesSchema } from '../lib/schemas';
import {
  findDevice,
  createChainHash,
  queryChainHashes,
  findAcceptedPartnership,
} from '../lib/db';

const hashes = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /hash — Upload a binary chain hash (32 bytes, application/octet-stream).
 * Headers: X-Device-ID, X-Client-Timestamp (ISO-8601)
 */
hashes.post('/', authenticate, async (c) => {
  const deviceId = c.req.header('X-Device-ID');
  if (!deviceId) return c.json({ error: 'X-Device-ID header required' }, 400);

  const clientTimestamp = c.req.header('X-Client-Timestamp');
  if (!clientTimestamp) return c.json({ error: 'X-Client-Timestamp header required' }, 400);

  const tsResult = z.iso.datetime().safeParse(clientTimestamp);
  if (!tsResult.success) return c.json({ error: 'X-Client-Timestamp must be ISO-8601' }, 400);

  const userId = c.get('userId');

  const device = await findDevice(c.env.DB, deviceId, userId);
  if (!device) return c.json({ error: 'Device not found' }, 404);

  // Read raw binary body — must be exactly 32 bytes
  const body = await c.req.arrayBuffer();
  if (body.byteLength !== 32) {
    return c.json({ error: 'Body must be exactly 32 bytes (SHA-256 hash)' }, 400);
  }

  const id = uuidv4();
  const createdAt = new Date().toISOString();

  await createChainHash(c.env.DB, id, userId, deviceId, body, clientTimestamp, createdAt);

  return c.json({ id, timestamp: clientTimestamp }, 201);
});

/**
 * GET /hash — Query chain hashes for tamper detection.
 * Query params: device_id, from (ISO), to (ISO), cursor?, limit?
 * Partners with view_data permission can query another user's hashes via ?user=<userId>.
 */
hashes.get('/', authenticate, async (c) => {
  const parsed = listHashesSchema.safeParse(c.req.query());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const requesterId = c.get('userId');
  const { device_id, from, to, cursor, limit } = parsed.data;

  // Optional ?user= for partner access
  const targetUser = c.req.query('user');
  const targetId = targetUser ?? requesterId;

  if (targetId !== requesterId) {
    const partnership = await findAcceptedPartnership(c.env.DB, targetId, requesterId);
    if (!partnership) return c.json({ error: 'Forbidden' }, 403);
    const perms = JSON.parse(partnership.permissions);
    if (!perms.view_data) return c.json({ error: 'Forbidden' }, 403);
  }

  const { items, hasMore } = await queryChainHashes(
    c.env.DB,
    targetId,
    device_id,
    from,
    to,
    cursor,
    limit,
  );

  // Convert raw BLOB to hex string for the client
  const mapped = items.map((item) => ({
    id: item.id,
    hash_hex: Buffer.from(item.hash).toString('hex'),
    client_timestamp: item.client_timestamp,
  }));

  return c.json({
    items: mapped,
    ...(hasMore && { next_cursor: items[items.length - 1].client_timestamp }),
  });
});

export default hashes;
