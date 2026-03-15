import { Hono } from 'hono';
import { cors } from 'hono/cors';
import auth from './routes/auth';
import data from './routes/data';
import deviceOnly from './routes/device-only';
import devices from './routes/devices';
import emailWebhooks from './routes/email-webhooks';
import hashes from './routes/hashes';
import notifications from './routes/notifications';
import partners from './routes/partners';
import { stripApiBasePath } from './lib/base-path';
import { runNotificationSchedule } from './lib/scheduler';
import { Env, Variables } from './types/bindings';

const app = new Hono<{ Bindings: Env; Variables: Variables }>({
  getPath: (request, options) =>
    stripApiBasePath(new URL(request.url).pathname, options?.env?.API_BASE_PATH),
});

app.use(
  '/*',
  cors({
    origin: (origin, c) => {
      const allowed = new URL(c.env.APP_URL || 'http://localhost:5173').origin;
      return origin === allowed ? origin : null;
    },
    allowMethods: ['GET', 'POST', 'PATCH', 'DELETE', 'OPTIONS'],
    allowHeaders: ['Content-Type', 'Authorization'],
    credentials: true,
  }),
);

app.get('/', (c) =>
  c.json({
    name: 'Virtue Initiative API',
    version: '1.0.0',
    status: 'ok',
  }),
);

app.route('/', auth);
app.route('/', notifications);
app.route('/', partners);
app.route('/', emailWebhooks);
app.route('/device', devices);
app.route('/data', data);
app.route('/d', deviceOnly);
app.route('/hash', hashes);

app.get('/r2/*', async (c) => {
  const key = c.req.path.replace(/^\/r2\//, '');
  const object = await c.env.BUCKET.get(key);

  if (!key || !object) {
    return c.json({ error: 'Not found' }, 404);
  }

  return new Response(object.body, {
    headers: { 'Content-Type': 'application/octet-stream' },
  });
});

app.onError((error, c) => {
  console.error(error);
  return c.json({ error: 'Internal server error', details: { message: error.message } }, 500);
});

app.notFound((c) => c.json({ error: 'Not found' }, 404));

export default {
  fetch: app.fetch,
  scheduled(controller: ScheduledController, env: Env, ctx: ExecutionContext) {
    ctx.waitUntil(runNotificationSchedule(env, controller.scheduledTime));
  },
};
