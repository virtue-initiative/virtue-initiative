import { Hono } from 'hono';
import { authenticate } from '../middleware/auth';
import { findDeviceById, getHashState, resetHashState, upsertHashState } from '../lib/db';
import { Env, Variables } from '../types/bindings';

const hashes = new Hono<{ Bindings: Env; Variables: Variables }>();
const ZERO_STATE = new Uint8Array(32);

hashes.post('/', authenticate('device-access'), async (c) => {
  const body = await c.req.arrayBuffer();

  if (body.byteLength !== 32) {
    return c.json({ error: 'Bad Request', details: { body: ['Expected exactly 32 bytes'] } }, 400);
  }

  const device = await findDeviceById(c.env.DB, c.get('sub'));
  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  const current = await getHashState(c.env.DB, device.id);
  const hashInput = new Uint8Array(64);
  hashInput.set(current ? new Uint8Array(current.state) : ZERO_STATE, 0);
  hashInput.set(new Uint8Array(body), 32);

  const nextState = await crypto.subtle.digest('SHA-256', hashInput);
  await upsertHashState(c.env.DB, {
    device_id: device.id,
    user_id: device.owner,
    state: nextState,
    updated_at: Date.now(),
  });

  return c.json({ ok: true });
});

hashes.get('/', authenticate('device-access'), async (c) => {
  const state = await getHashState(c.env.DB, c.get('sub'));
  const body = state ? new Uint8Array(state.state) : ZERO_STATE;

  return new Response(body, {
    headers: { 'Content-Type': 'application/octet-stream' },
  });
});

hashes.delete('/', authenticate('server'), async (c) => {
  const device = await findDeviceById(c.env.DB, c.get('sub'));
  if (!device) {
    return c.json({ error: 'Not found' }, 404);
  }

  await resetHashState(c.env.DB, device.id, device.owner, Date.now());
  return c.json({ ok: true });
});

export default hashes;
