import { Hono } from 'hono';
import { cors } from 'hono/cors';
import { Env, Variables } from './types/bindings';
import auth from './routes/auth';
import batches from './routes/batches';
import hashes from './routes/hashes';
import hashServer from './routes/hash-server';
import devices from './routes/devices';
import partners from './routes/partners';
import settings from './routes/settings';

const app = new Hono<{ Bindings: Env; Variables: Variables }>();

app.use(
  '/*',
  cors({
    origin: (origin, c) => {
      const allowed = c.env.CORS_ORIGIN || 'http://localhost:5173';
      return origin === allowed ? origin : null;
    },
    allowMethods: ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS'],
    allowHeaders: ['Content-Type', 'Authorization'],
    credentials: true,
  }),
);

// Health check
app.get('/', (c) => {
  return c.json({
    name: 'BePure API',
    version: '1.0.0',
    status: 'ok',
  });
});

// Mount routes
app.route('/', auth); // auth handles /signup, /login, /logout, /token at its own paths
app.route('/batch', batches);
app.route('/hash', hashes);
app.route('/device', devices);
app.route('/partner', partners);
app.route('/settings', settings);
app.route('/hash-server', hashServer);

// Public R2 pass-through — blobs are E2EE encrypted so no auth needed.
// In production replace VITE_R2_URL with the real public R2 bucket URL.
app.get('/r2/*', async (c) => {
  const key = c.req.path.replace(/^\/r2\//, '');
  if (!key) return c.json({ error: 'Not found' }, 404);
  const obj = await c.env.BUCKET.get(key);
  if (!obj) return c.json({ error: 'Not found' }, 404);
  return new Response(obj.body, {
    headers: { 'Content-Type': 'application/octet-stream' },
  });
});

// Error handling
app.onError((err, c) => {
  console.error('Unhandled error:', err);
  return c.json(
    {
      error: 'Internal server error',
      message: err.message,
    },
    500,
  );
});

// 404 handler
app.notFound((c) => {
  return c.json({ error: 'Not found' }, 404);
});

export default app;
