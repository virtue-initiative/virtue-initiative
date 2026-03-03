import z from 'zod';
import { Context, Hono } from 'hono';
import { setCookie, deleteCookie, getCookie } from 'hono/cookie';
import { v4 as uuidv4 } from 'uuid';
import { Env, Variables } from '../types/bindings';
import { hashPassword, verifyPassword } from '../lib/password';
import { generateAccessToken, generateRefreshToken, verifyJWT } from '../lib/jwt';
import { signupSchema, loginSchema } from '../lib/schemas';
import { findUserByEmail, createUser, updateUser, findUserById } from '../lib/db';
import { authenticate } from '../middleware/auth';

const ACCESS_EXPIRY = 60 * 60;
const REFRESH_EXPIRY = 365 * 24 * 60 * 60;

const auth = new Hono<{ Bindings: Env; Variables: Variables }>();

async function createSession(userId: string, c: Context<{ Bindings: Env; Variables: Variables }>) {
  const accessToken = await generateAccessToken(userId, c.env.JWT_SECRET, ACCESS_EXPIRY);
  const refreshToken = await generateRefreshToken(userId, c.env.JWT_SECRET, REFRESH_EXPIRY);

  setCookie(c, 'refresh_token', refreshToken, {
    httpOnly: true,
    secure: true,
    sameSite: 'Strict',
    maxAge: REFRESH_EXPIRY,
    path: '/',
  });

  return accessToken;
}

/**
 * POST /signup - Create new user account
 */
auth.post('/signup', async (c) => {
  const parsed = signupSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const { email, password, name } = parsed.data;

  const existingUser = await findUserByEmail(c.env.DB, email);
  if (existingUser) return c.json({ error: 'User already exists' }, 409);

  const passwordHash = await hashPassword(password);
  const userId = uuidv4();
  const createdAt = new Date().toISOString();

  await createUser(c.env.DB, userId, email, passwordHash, name ?? null, createdAt);

  const accessToken = await createSession(userId, c);

  return c.json(
    { user: { id: userId, email, created_at: createdAt }, access_token: accessToken },
    201,
  );
});

/**
 * POST /login - Authenticate user
 */
auth.post('/login', async (c) => {
  const parsed = loginSchema.safeParse(await c.req.json());
  if (!parsed.success) return c.json({ error: z.treeifyError(parsed.error) }, 400);

  const { email, password } = parsed.data;

  const user = await findUserByEmail(c.env.DB, email);

  if (!user || !(await verifyPassword(password, user.password_hash))) {
    return c.json({ error: 'Invalid credentials' }, 401);
  }

  const accessToken = await createSession(user.id, c);

  return c.json({ access_token: accessToken });
});

/**
 * POST /logout - Clear refresh token
 */
auth.post('/logout', async (c) => {
  deleteCookie(c, 'refresh_token', { path: '/' });
  return c.body(null, 204);
});

/**
 * POST /token - Refresh access token from cookie
 */
auth.post('/token', async (c) => {
  const refreshToken = getCookie(c, 'refresh_token');
  if (!refreshToken) return c.json({ error: 'No refresh token found' }, 401);

  try {
    const payload = await verifyJWT(refreshToken, c.env.JWT_SECRET);
    if (payload.type !== 'refresh') return c.json({ error: 'Invalid token type' }, 401);

    const accessToken = await generateAccessToken(payload.sub, c.env.JWT_SECRET, ACCESS_EXPIRY);
    return c.json({ access_token: accessToken }, 201);
  } catch {
    return c.json({ error: 'Invalid or expired refresh token' }, 401);
  }
});

auth.post('/e2ee', authenticate, async (c) => {
  const userId = c.get('userId');
  const body = await c.req.json();
  if (typeof body?.encrypted_key !== 'string' || !body.encrypted_key) {
    return c.json({ error: 'encrypted_key is required' }, 400);
  }
  const decoded = Uint8Array.fromBase64(body.encrypted_key);
  await updateUser(c.env.DB, userId, { e2ee_key: decoded.buffer });
  return c.json({ encrypted_key: body.encrypted_key });
});

auth.get('/e2ee', authenticate, async (c) => {
  const userId = c.get('userId');
  const user = await findUserById(c.env.DB, userId);
  if (!user?.e2ee_key) return c.json({ encrypted_key: null });
  return c.json({ encrypted_key: new Uint8Array(user.e2ee_key).toBase64() });
});

export default auth;
