import { Hono } from 'hono';
import { authenticate } from '../middleware/auth';
import { rateLimitByDevice } from '../middleware/rate-limit';
import { localHashGet, localHashIngest, localHashInfo, localHashReset } from '../lib/hash-server';
import { Env, Variables } from '../types/bindings';

const hashes = new Hono<{ Bindings: Env; Variables: Variables }>();

hashes.post('/', authenticate('hash-server'), rateLimitByDevice(), async (c) => {
  const body = await c.req.arrayBuffer();

  if (body.byteLength !== 32) {
    return c.json({ error: 'Bad Request', details: { body: ['Expected exactly 32 bytes'] } }, 400);
  }

  await localHashIngest(c.env.DB, c.get('sub'), new Uint8Array(body));

  return c.json({ ok: true });
});

hashes.get('/', authenticate('hash-server'), rateLimitByDevice(), async (c) => {
  const body = await localHashGet(c.env.DB, c.get('sub'));

  return new Response(body, {
    headers: { 'Content-Type': 'application/octet-stream' },
  });
});

hashes.delete('/', authenticate('server'), async (c) => {
  await localHashReset(c.env.DB, c.get('sub'));
  return c.json({ ok: true });
});

hashes.get('/info', authenticate(['hash-server', 'server']), async (c) => {
  const info = await localHashInfo(c.env.DB, c.get('sub'));
  return c.json({
    count: info?.count ?? 0,
    hashed_at: info?.hashed_at ?? null,
    updated_at: info?.updated_at ?? null,
  });
});

export default hashes;
