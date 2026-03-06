import { Context, Hono } from 'hono';
import { getCookie, deleteCookie, setCookie } from 'hono/cookie';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import { createUser, findUserByEmail, findUserById, updateUser } from '../lib/db';
import { decodeBase64, encodeBase64 } from '../lib/encoding';
import { generateAccessToken, generateRefreshToken, verifyJWT } from '../lib/jwt';
import { hashPassword, verifyPassword } from '../lib/password';
import { Env, Variables } from '../types/bindings';

const auth = new Hono<{ Bindings: Env; Variables: Variables }>();
const ACCESS_TOKEN_TTL_SECONDS = 60 * 60;
const REFRESH_TOKEN_TTL_SECONDS = 365 * 24 * 60 * 60;

const signupSchema = z.object({
  email: z.email(),
  password: z.string().min(1),
  name: z.string().min(1).optional(),
});

const loginSchema = z.object({
  email: z.email(),
  password: z.string().min(1),
});

const updateUserSchema = z
  .object({
    name: z.string().min(1).optional(),
    e2ee_key: z.base64().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

async function createSession(c: Context<{ Bindings: Env; Variables: Variables }>, userId: string) {
  const accessToken = await generateAccessToken(userId, c.env.JWT_SECRET, ACCESS_TOKEN_TTL_SECONDS);
  const refreshToken = await generateRefreshToken(
    userId,
    c.env.JWT_SECRET,
    REFRESH_TOKEN_TTL_SECONDS,
  );

  setCookie(c, 'refresh_token', refreshToken, {
    httpOnly: true,
    sameSite: 'Strict',
    secure: true,
    path: '/',
    maxAge: REFRESH_TOKEN_TTL_SECONDS,
  });

  return accessToken;
}

auth.post('/signup', validateZ('json', signupSchema), async (c) => {
  const { email, password, name } = c.req.valid('json');
  const existingUser = await findUserByEmail(c.env.DB, email);

  if (existingUser) {
    return c.json({ error: 'User already exists' }, 409);
  }

  const userId = uuidv4();
  const passwordHash = await hashPassword(password);

  await createUser(c.env.DB, { id: userId, email, passwordHash, name });
  const accessToken = await createSession(c, userId);

  return c.json(
    {
      user: { id: userId, email, ...(name ? { name } : {}) },
      access_token: accessToken,
    },
    201,
  );
});

auth.post('/login', validateZ('json', loginSchema), async (c) => {
  const { email, password } = c.req.valid('json');
  const user = await findUserByEmail(c.env.DB, email);

  if (!user || !(await verifyPassword(password, user.password_hash))) {
    return c.json({ error: 'Unauthorized' }, 401);
  }

  const accessToken = await createSession(c, user.id);
  return c.json({ access_token: accessToken });
});

auth.post('/logout', async (c) => {
  deleteCookie(c, 'refresh_token', { path: '/' });
  return c.body(null, 204);
});

auth.post('/token', async (c) => {
  const refreshToken = getCookie(c, 'refresh_token');

  if (!refreshToken) {
    return c.json({ error: 'Unauthorized' }, 401);
  }

  try {
    const payload = await verifyJWT(refreshToken, c.env.JWT_SECRET);

    if (payload.type !== 'refresh') {
      return c.json({ error: 'Unauthorized' }, 401);
    }

    const accessToken = await generateAccessToken(
      payload.sub,
      c.env.JWT_SECRET,
      ACCESS_TOKEN_TTL_SECONDS,
    );

    return c.json({ access_token: accessToken }, 201);
  } catch {
    return c.json({ error: 'Unauthorized' }, 401);
  }
});

auth.get('/user', authenticate('access'), async (c) => {
  const user = await findUserById(c.env.DB, c.get('sub'));

  if (!user) {
    return c.json({ error: 'Not found' }, 404);
  }

  return c.json({
    id: user.id,
    email: user.email,
    ...(user.name ? { name: user.name } : {}),
    ...(user.e2ee_key ? { e2ee_key: encodeBase64(user.e2ee_key) } : {}),
  });
});

auth.patch('/user', authenticate('access'), validateZ('json', updateUserSchema), async (c) => {
  const { name, e2ee_key } = c.req.valid('json');

  await updateUser(c.env.DB, c.get('sub'), {
    name,
    e2ee_key: e2ee_key ? decodeBase64(e2ee_key) : undefined,
  });

  return c.json({ ok: true });
});

export default auth;
