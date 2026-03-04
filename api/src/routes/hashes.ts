import { Hono } from 'hono';
import z from 'zod';
import { Env, Variables } from '../types/bindings';
import { authenticate } from '../middleware/auth';
import { getStateSchema } from '../lib/schemas';
import { findDevice, getDeviceState, upsertDeviceState, findAcceptedPartnership } from '../lib/db';

const hashes = new Hono<{ Bindings: Env; Variables: Variables }>();

const ZEROS = new Uint8Array(32);

/** Converts a 16-byte Uint8Array to a lowercase UUID string (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx). */
function bytesToUuid(b: Uint8Array): string {
  const h = Array.from(b)
    .map((x) => x.toString(16).padStart(2, '0'))
    .join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

/**
 * POST /hash — Upload a log's content hash; server computes and stores the new state.
 * Body: exactly 48 bytes (application/octet-stream)
 *   [0..16)  device_id as raw UUID bytes
 *   [16..48) content_hash (SHA-256 of the plaintext log item)
 *
 * Server computes: new_state = sha256(current_state || content_hash) and stores it.
 */
hashes.post('/', authenticate, async (c) => {
  const body = await c.req.arrayBuffer();
  if (body.byteLength !== 48) {
    return c.json({ error: 'Body must be exactly 48 bytes (16 device_id + 32 content_hash)' }, 400);
  }

  const buf = new Uint8Array(body);
  const deviceIdBytes = buf.slice(0, 16);
  const contentHash = buf.slice(16, 48);
  const deviceId = bytesToUuid(deviceIdBytes);

  const userId = c.get('userId');
  const device = await findDevice(c.env.DB, deviceId, userId);
  if (!device) return c.json({ error: 'Device not found' }, 404);

  // Retrieve current state (default to 32 zero bytes for first upload)
  const existing = await getDeviceState(c.env.DB, deviceId);
  const currentState = existing ? new Uint8Array(existing.state) : ZEROS;

  // Compute new_state = sha256(current_state || content_hash)
  const hashInput = new Uint8Array(64);
  hashInput.set(currentState, 0);
  hashInput.set(contentHash, 32);
  const newState = new Uint8Array(await crypto.subtle.digest('SHA-256', hashInput));

  const updatedAt = new Date().toISOString();
  await upsertDeviceState(c.env.DB, deviceId, userId, newState.buffer, updatedAt);

  return c.json({ ok: true }, 200);
});

/**
 * GET /hash — Get the current rolling state for a device.
 * Query params: device_id (required), user (optional, for partner access)
 */
hashes.get('/', authenticate, async (c) => {
  const parsed = getStateSchema.safeParse(c.req.query());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const requesterId = c.get('userId');
  const { device_id, user: targetUser } = parsed.data;
  const targetId = targetUser ?? requesterId;

  if (targetId !== requesterId) {
    const partnership = await findAcceptedPartnership(c.env.DB, targetId, requesterId);
    if (!partnership) return c.json({ error: 'Forbidden' }, 403);
    const perms = JSON.parse(partnership.permissions);
    if (!perms.view_data) return c.json({ error: 'Forbidden' }, 403);
  }

  const existing = await getDeviceState(c.env.DB, device_id);
  const stateHex = existing ? Buffer.from(existing.state).toString('hex') : '0'.repeat(64);

  return c.json({ state_hex: stateHex });
});

export default hashes;
