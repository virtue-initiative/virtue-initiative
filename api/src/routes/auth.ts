import { Context, Hono } from 'hono';
import { getCookie, deleteCookie, setCookie } from 'hono/cookie';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  createSessionRecord,
  createEmailToken,
  createUser,
  deleteSessionByRefreshTokenHash,
  findEmailTokenByHash,
  findSessionByRefreshTokenHash,
  findUserByEmail,
  findUserById,
  invalidateEmailTokens,
  updateUser,
  consumeEmailToken,
} from '../lib/db';
import {
  renderEmailVerificationTemplate,
  renderPasswordResetTemplate,
} from '../lib/email/templates';
import { sendEmail } from '../lib/email';
import { decodeBase64, encodeBase64 } from '../lib/encoding';
import { EMAIL_VERIFICATION_TTL_MS, PASSWORD_RESET_TTL_MS } from '../lib/email-domain';
import { generateAccessToken } from '../lib/jwt';
import {
  CURRENT_HASH_PARAMS,
  HASH_PARAMS_VERSION,
  generatePasswordSalt,
  hashPasswordAuth,
  verifyPasswordAuth,
} from '../lib/password';
import { generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { Env, Variables } from '../types/bindings';

const auth = new Hono<{ Bindings: Env; Variables: Variables }>();
const ACCESS_TOKEN_TTL_SECONDS = 60 * 60;
const REFRESH_TOKEN_TTL_SECONDS = 365 * 24 * 60 * 60;
const LOCAL_WEB_URL = 'http://localhost:5173';

const signupSchema = z.object({
  email: z.email(),
  password_auth: z.base64(),
  password_salt: z.base64(),
  pub_key: z.base64(),
  priv_key: z.base64(),
  name: z.string().min(1).optional(),
});

const loginMaterialQuerySchema = z.object({
  email: z.email(),
});

const loginSchema = z.object({
  email: z.email(),
  password_auth: z.base64(),
});

const verifyEmailSchema = z.object({
  token: z.string().min(1),
});

const passwordResetRequestSchema = z.object({
  email: z.email(),
});

const passwordResetValidateSchema = z.object({
  token: z.string().min(1),
});

const passwordResetSchema = z.object({
  token: z.string().min(1),
  password_auth: z.base64(),
  password_salt: z.base64(),
  pub_key: z.base64(),
  priv_key: z.base64(),
});

const updateUserSchema = z
  .object({
    email: z.email().optional(),
    name: z.string().min(1).optional(),
    pub_key: z.base64().optional(),
    priv_key: z.base64().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

function buildHashParamsResponse() {
  return {
    version: CURRENT_HASH_PARAMS.version,
    algorithm: CURRENT_HASH_PARAMS.algorithm,
    memory_cost_kib: CURRENT_HASH_PARAMS.memory_cost_kib,
    time_cost: CURRENT_HASH_PARAMS.time_cost,
    parallelism: CURRENT_HASH_PARAMS.parallelism,
    salt_length: CURRENT_HASH_PARAMS.salt_length,
    hkdf_hash: CURRENT_HASH_PARAMS.hkdf_hash,
  };
}

function decodeRequiredBase64(value: string, field: string) {
  const decoded = decodeBase64(value);
  if (new Uint8Array(decoded).byteLength === 0) {
    throw new Error(`${field} must not be empty`);
  }
  return decoded;
}

function decodePasswordSalt(value: string) {
  const decoded = decodeRequiredBase64(value, 'password_salt');
  if (new Uint8Array(decoded).byteLength !== CURRENT_HASH_PARAMS.salt_length) {
    throw new Error(`password_salt must be ${CURRENT_HASH_PARAMS.salt_length} bytes`);
  }
  return decoded;
}

function decodePasswordAuth(value: string) {
  const decoded = decodeRequiredBase64(value, 'password_auth');
  if (new Uint8Array(decoded).byteLength !== 32) {
    throw new Error('password_auth must be 32 bytes');
  }
  return decoded;
}

function decodePublicKey(value: string) {
  const decoded = decodeRequiredBase64(value, 'pub_key');
  if (new Uint8Array(decoded).byteLength !== 32) {
    throw new Error('pub_key must be 32 bytes');
  }
  return decoded;
}

function badRequest(c: Context<{ Bindings: Env; Variables: Variables }>, message: string) {
  return c.json({ error: 'Bad Request', details: { errors: [message] } }, 400);
}

async function createSession(c: Context<{ Bindings: Env; Variables: Variables }>, userId: string) {
  const accessToken = await generateAccessToken(userId, c.env.JWT_SECRET, ACCESS_TOKEN_TTL_SECONDS);
  const refreshToken = generateOpaqueToken();
  const now = Date.now();

  await createSessionRecord(c.env.DB, {
    session_type: 'web',
    user_id: userId,
    refresh_token_hash: hashOpaqueToken(refreshToken),
    expires_at: now + REFRESH_TOKEN_TTL_SECONDS * 1000,
    created_at: now,
  });

  setCookie(c, 'refresh_token', refreshToken, {
    httpOnly: true,
    sameSite: 'Strict',
    secure: true,
    path: '/',
    maxAge: REFRESH_TOKEN_TTL_SECONDS,
  });

  return accessToken;
}

function getAppUrl(c: Context<{ Bindings: Env; Variables: Variables }>) {
  const requestUrl = new URL(c.req.url);
  if (requestUrl.hostname === 'localhost' || requestUrl.hostname === '127.0.0.1') {
    return LOCAL_WEB_URL;
  }

  return c.env.APP_URL;
}

async function issueEmailToken(
  db: D1Database,
  user: { id: string; email: string },
  purpose: 'email_verification' | 'password_reset',
  ttlMs: number,
) {
  await invalidateEmailTokens(db, user.id, purpose);
  const token = generateOpaqueToken();
  const now = Date.now();
  await createEmailToken(db, {
    id: uuidv4(),
    user_id: user.id,
    email: user.email,
    purpose,
    token_hash: hashOpaqueToken(token),
    expires_at: now + ttlMs,
    created_at: now,
  });
  return token;
}

async function sendVerificationEmail(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  user: { id: string; email: string; name?: string | null },
) {
  const token = await issueEmailToken(
    c.env.DB,
    user,
    'email_verification',
    EMAIL_VERIFICATION_TTL_MS,
  );
  const verifyUrl = `${getAppUrl(c)}/?verify_email_token=${encodeURIComponent(token)}`;
  const email = renderEmailVerificationTemplate({
    appName: c.env.APP_NAME,
    appUrl: getAppUrl(c),
    recipientName: user.name,
    verifyUrl,
  });

  await sendEmail({
    env: c.env,
    db: c.env.DB,
    kind: 'email_verification',
    recipient: user.email,
    subject: email.subject,
    text: email.text,
    html: email.html,
    related_user_id: user.id,
    metadata: { purpose: 'email_verification', verifyUrl },
  });
}

async function sendPasswordResetEmail(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  user: { id: string; email: string; name?: string | null },
) {
  const token = await issueEmailToken(c.env.DB, user, 'password_reset', PASSWORD_RESET_TTL_MS);
  const resetUrl = `${getAppUrl(c)}/?reset_password_token=${encodeURIComponent(token)}`;
  const email = renderPasswordResetTemplate({
    appName: c.env.APP_NAME,
    appUrl: getAppUrl(c),
    recipientName: user.name,
    resetUrl,
  });

  await sendEmail({
    env: c.env,
    db: c.env.DB,
    kind: 'password_reset',
    recipient: user.email,
    subject: email.subject,
    text: email.text,
    html: email.html,
    related_user_id: user.id,
    metadata: { purpose: 'password_reset', resetUrl },
  });
}

async function getValidTokenRecord(
  db: D1Database,
  rawToken: string,
  purpose: 'email_verification' | 'password_reset',
) {
  const token = await findEmailTokenByHash(db, hashOpaqueToken(rawToken), purpose);
  if (!token || !token.user_id || token.consumed_at || token.expires_at < Date.now()) {
    return null;
  }
  return { ...token, user_id: token.user_id };
}

auth.get('/current-hash-params', async (c) => c.json(buildHashParamsResponse()));

auth.get('/user/login-material', validateZ('query', loginMaterialQuerySchema), async (c) => {
  const { email } = c.req.valid('query');
  const user = await findUserByEmail(c.env.DB, email.trim().toLowerCase());

  return c.json({
    password_salt: encodeBase64(user?.password_salt ?? generatePasswordSalt()),
    params: buildHashParamsResponse(),
  });
});

auth.post('/signup', validateZ('json', signupSchema), async (c) => {
  const { email, password_auth, password_salt, pub_key, priv_key, name } = c.req.valid('json');
  const normalizedEmail = email.trim().toLowerCase();
  const existingUser = await findUserByEmail(c.env.DB, normalizedEmail);

  if (existingUser) {
    return c.json({ error: 'User already exists' }, 409);
  }

  let decodedPasswordAuth: ArrayBuffer;
  let decodedPasswordSalt: ArrayBuffer;
  let decodedPublicKey: ArrayBuffer;
  let decodedPrivateKey: ArrayBuffer;

  try {
    decodedPasswordAuth = decodePasswordAuth(password_auth);
    decodedPasswordSalt = decodePasswordSalt(password_salt);
    decodedPublicKey = decodePublicKey(pub_key);
    decodedPrivateKey = decodeRequiredBase64(priv_key, 'priv_key');
  } catch (error) {
    return badRequest(c, error instanceof Error ? error.message : 'Invalid signup payload');
  }

  const userId = uuidv4();
  const passwordHash = await hashPasswordAuth(decodedPasswordAuth);

  await createUser(c.env.DB, {
    id: userId,
    email: normalizedEmail,
    passwordHash,
    passwordSalt: decodedPasswordSalt,
    passwordParamsVersion: HASH_PARAMS_VERSION,
    pub_key: decodedPublicKey,
    priv_key: decodedPrivateKey,
    name,
  });
  await sendVerificationEmail(c, { id: userId, email: normalizedEmail, name });
  const accessToken = await createSession(c, userId);

  return c.json(
    {
      user: {
        id: userId,
        email: normalizedEmail,
        email_verified: false,
        ...(name ? { name } : {}),
      },
      access_token: accessToken,
    },
    201,
  );
});

auth.post('/login', validateZ('json', loginSchema), async (c) => {
  const { email, password_auth } = c.req.valid('json');
  const normalizedEmail = email.trim().toLowerCase();
  const user = await findUserByEmail(c.env.DB, normalizedEmail);

  let decodedPasswordAuth: ArrayBuffer;
  try {
    decodedPasswordAuth = decodePasswordAuth(password_auth);
  } catch {
    return c.json({ error: 'Invalid email or password' }, 401);
  }

  if (!user || !(await verifyPasswordAuth(decodedPasswordAuth, user.password_hash))) {
    return c.json({ error: 'Invalid email or password' }, 401);
  }

  const accessToken = await createSession(c, user.id);
  return c.json({ access_token: accessToken });
});

auth.post('/logout', async (c) => {
  const refreshToken = getCookie(c, 'refresh_token');
  if (refreshToken) {
    await deleteSessionByRefreshTokenHash(c.env.DB, hashOpaqueToken(refreshToken), 'web');
  }
  deleteCookie(c, 'refresh_token', { path: '/' });
  return c.body(null, 204);
});

auth.post('/token', async (c) => {
  const refreshToken = getCookie(c, 'refresh_token');

  if (!refreshToken) {
    return c.json({ error: 'Session expired. Please log in again.' }, 401);
  }

  const session = await findSessionByRefreshTokenHash(
    c.env.DB,
    hashOpaqueToken(refreshToken),
    'web',
  );

  if (!session || !session.user_id) {
    return c.json({ error: 'Unauthorized' }, 401);
  }

  if (session.expires_at < Date.now()) {
    await deleteSessionByRefreshTokenHash(c.env.DB, session.refresh_token_hash, 'web');
    return c.json({ error: 'Unauthorized' }, 401);
  }

  const accessToken = await generateAccessToken(
    session.user_id,
    c.env.JWT_SECRET,
    ACCESS_TOKEN_TTL_SECONDS,
  );

  return c.json({ access_token: accessToken }, 201);
});

auth.get('/user', authenticate('access'), async (c) => {
  const user = await findUserById(c.env.DB, c.get('sub'));

  if (!user) {
    return c.json({ error: 'User account not found' }, 404);
  }

  return c.json({
    id: user.id,
    email: user.email,
    email_verified: user.email_verified === 1,
    email_bounced_at: user.email_bounced_at,
    ...(user.name ? { name: user.name } : {}),
    ...(user.pub_key ? { pub_key: encodeBase64(user.pub_key) } : {}),
    ...(user.priv_key ? { priv_key: encodeBase64(user.priv_key) } : {}),
  });
});

auth.patch('/user', authenticate('access'), validateZ('json', updateUserSchema), async (c) => {
  const userId = c.get('sub');
  const { email, name, pub_key, priv_key } = c.req.valid('json');
  const normalizedEmail = email?.trim().toLowerCase();
  const user = await findUserById(c.env.DB, userId);

  if (!user) {
    return c.json({ error: 'User account not found' }, 404);
  }

  const emailChanged = Boolean(normalizedEmail && normalizedEmail !== user.email);
  if (emailChanged) {
    const existingUser = await findUserByEmail(c.env.DB, normalizedEmail!);
    if (existingUser && existingUser.id !== userId) {
      return c.json({ error: 'Email is already in use' }, 409);
    }
  }

  let decodedPublicKey: ArrayBuffer | undefined;
  let decodedPrivateKey: ArrayBuffer | undefined;

  try {
    decodedPublicKey = pub_key ? decodePublicKey(pub_key) : undefined;
    decodedPrivateKey = priv_key ? decodeRequiredBase64(priv_key, 'priv_key') : undefined;
  } catch (error) {
    return badRequest(c, error instanceof Error ? error.message : 'Invalid user update payload');
  }

  await updateUser(c.env.DB, userId, {
    ...(emailChanged
      ? { email: normalizedEmail, email_verified: false, email_bounced_at: null }
      : {}),
    name,
    pub_key: decodedPublicKey,
    priv_key: decodedPrivateKey,
  });

  return c.json({ ok: true });
});

auth.post('/email-verification/validate', validateZ('json', verifyEmailSchema), async (c) => {
  const { token } = c.req.valid('json');
  const record = await getValidTokenRecord(c.env.DB, token, 'email_verification');

  if (!record) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  await updateUser(c.env.DB, record.user_id, { email_verified: true, email_bounced_at: null });
  await consumeEmailToken(c.env.DB, record.id, Date.now());
  await invalidateEmailTokens(c.env.DB, record.user_id, 'email_verification');

  return c.json({ ok: true, email: record.email });
});

auth.post('/email-verification', authenticate('access'), async (c) => {
  const user = await findUserById(c.env.DB, c.get('sub'));

  if (!user) {
    return c.json({ error: 'User account not found' }, 404);
  }

  if (user.email_verified === 1) {
    return c.json({ ok: true, already_verified: true });
  }

  if (user.email_bounced_at) {
    return c.json(
      {
        error:
          'Your last verification email bounced. Please update your email address before requesting another verification email.',
      },
      409,
    );
  }

  await sendVerificationEmail(c, user);
  return c.json({ ok: true });
});

auth.post('/password-reset', validateZ('json', passwordResetRequestSchema), async (c) => {
  const { email } = c.req.valid('json');
  const user = await findUserByEmail(c.env.DB, email.trim().toLowerCase());

  if (user) {
    await sendPasswordResetEmail(c, user);
  }

  return c.body(null, 204);
});

auth.post('/password-reset/validate', validateZ('json', passwordResetValidateSchema), async (c) => {
  const { token } = c.req.valid('json');
  const record = await getValidTokenRecord(c.env.DB, token, 'password_reset');

  if (!record) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  const user = await findUserById(c.env.DB, record.user_id);
  if (!user) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  return c.json({
    ok: true,
    email: record.email,
  });
});

auth.post('/password-reset/finalize', validateZ('json', passwordResetSchema), async (c) => {
  const { token, password_auth, password_salt, pub_key, priv_key } = c.req.valid('json');
  const record = await getValidTokenRecord(c.env.DB, token, 'password_reset');

  if (!record) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  const user = await findUserById(c.env.DB, record.user_id);
  if (!user) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  let decodedPasswordAuth: ArrayBuffer;
  let decodedPasswordSalt: ArrayBuffer;
  let decodedPublicKey: ArrayBuffer;
  let decodedPrivateKey: ArrayBuffer;

  try {
    decodedPasswordAuth = decodePasswordAuth(password_auth);
    decodedPasswordSalt = decodePasswordSalt(password_salt);
    decodedPublicKey = decodePublicKey(pub_key);
    decodedPrivateKey = decodeRequiredBase64(priv_key, 'priv_key');
  } catch (error) {
    return badRequest(c, error instanceof Error ? error.message : 'Invalid password reset payload');
  }

  await updateUser(c.env.DB, record.user_id, {
    password_hash: await hashPasswordAuth(decodedPasswordAuth),
    password_salt: decodedPasswordSalt,
    password_params_version: HASH_PARAMS_VERSION,
    pub_key: decodedPublicKey,
    priv_key: decodedPrivateKey,
  });
  await consumeEmailToken(c.env.DB, record.id, Date.now());
  await invalidateEmailTokens(c.env.DB, record.user_id, 'password_reset');

  return c.json({ ok: true });
});

export default auth;
