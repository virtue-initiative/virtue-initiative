import { Hono } from 'hono';
import { cors } from 'hono/cors';
import { Env, Variables } from './types/bindings';
import auth from './routes/auth';
import images from './routes/images';
import logs from './routes/logs';
import devices from './routes/devices';
import partners from './routes/partners';
import settings from './routes/settings';

const app = new Hono<{ Bindings: Env; Variables: Variables }>();

// Also remove X-API-Key from allowed headers since devices now use JWT
app.use(
  '/*',
  cors({
    origin: '*', // Configure appropriately for production
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
app.route('/image', images);
app.route('/log', logs);
app.route('/device', devices);
app.route('/partner', partners);
app.route('/settings', settings);

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
