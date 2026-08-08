import { Hono } from 'hono';
import { cors } from 'hono/cors';
import auth from './routes/auth';
import data from './routes/data';
import deviceOnly from './routes/device-only';
import devices from './routes/devices';
import emailWebhooks from './routes/email-webhooks';
import partners from './routes/partners';
import { stripApiBasePath } from './lib/base-path';
import { getJWKS } from './lib/jwt';
import {
  pruneExpiredBatches,
  pruneExpiredDeviceSessions,
  pruneExpiredEmailTokens,
  pruneExpiredUserSessions,
} from './lib/retention';
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
      const allowedOrigins = [c.env.APP_URL, 'http://localhost:5173'].map(
        (url) => new URL(url).origin,
      );
      return allowedOrigins.find((o) => o === origin);
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

app.get('/.well-known/jwks.json', async (c) => c.json(await getJWKS(c.env.JWT_PUBLIC_KEY)));

app.route('/', auth);
app.route('/', partners);
app.route('/', emailWebhooks);
app.route('/device', devices);
app.route('/data', data);
app.route('/d', deviceOnly);

app.get('/r2/*', async (c) => {
  const key = c.req.path.replace(/^\/r2\//, '');
  const object = await c.env.BUCKET.get(key);

  if (!key || !object) {
    return c.json({ error: 'Not found' }, 404);
  }

  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set('etag', object.httpEtag);

  return new Response(object.body, {
    headers,
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
    ctx.waitUntil(pruneExpiredBatches(env, controller.scheduledTime));
    ctx.waitUntil(pruneExpiredEmailTokens(env, controller.scheduledTime));
    ctx.waitUntil(pruneExpiredUserSessions(env, controller.scheduledTime));
    ctx.waitUntil(pruneExpiredDeviceSessions(env, controller.scheduledTime));
  },
};
