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

  const allowedOrigins = [c.env.LANDING_URL, 'http://localhost:4321'].map(
    (url) => new URL(url).origin,
  );

  return cors({
    origin: (origin) => allowedOrigins.find((o) => o === origin),
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
