import { Hono } from 'hono';
import { cors } from 'hono/cors';
import checkout from './routes/checkout';
import webhook from './routes/webhook';
import { Env, Variables } from './types/bindings';

const app = new Hono<{ Bindings: Env; Variables: Variables }>();

app.use('/*', async (c, next) => {
  // The webhook is called server-to-server by Stripe and must not be CORS-gated.
  if (c.req.path === '/webhook') {
    return next();
  }

  const allowedOrigins = [c.env.LANDING_URL].map((url) => new URL(url).origin);

  // Local dev serves the landing site from an unpredictable origin: either a
  // random Vite port (http://localhost:PORT) or a Caddy vanity host
  // (https://<domain>.localhost). `.localhost` is a reserved loopback TLD that
  // can only ever originate from the developer's own machine, so allowing any
  // localhost origin is safe and avoids CORS breaking on port/host changes.
  const isLocalhostOrigin = (origin: string) => {
    try {
      const { hostname } = new URL(origin);
      return hostname === 'localhost' || hostname.endsWith('.localhost');
    } catch {
      return false;
    }
  };

  return cors({
    origin: (origin) =>
      allowedOrigins.includes(origin) || isLocalhostOrigin(origin) ? origin : null,
    allowMethods: ['GET', 'POST', 'OPTIONS'],
    allowHeaders: ['Content-Type'],
  })(c, next);
});

app.get('/', (c) =>
  c.json({
    name: 'Virtue Initiative Donations API',
    version: '1.0.0',
    status: 'ok',
  }),
);

app.route('/', checkout);
app.route('/', webhook);

app.onError((error, c) => {
  console.error(error);
  return c.json({ error: 'Internal server error', details: { message: error.message } }, 500);
});

app.notFound((c) => c.json({ error: 'Not found' }, 404));

export default app;
