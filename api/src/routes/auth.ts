import { Context, Hono } from 'hono';
import { getCookie, deleteCookie, setCookie } from 'hono/cookie';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticate } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  clearPartnerAccessKeysForUser,
  createSessionRecord,
  createEmailToken,
  createUser,
  deleteSessionByRefreshTokenHash,
  findEmailTokenByHash,
  findSessionByRefreshTokenHash,
  findUserByEmail,
  findUserById,
  invalidateEmailTokens,
  listPartnerAccessTargetsForOwner,
  updateUser,
  updatePartnerAccessKeys,
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
import { hashPassword, verifyPassword } from '../lib/password';
import { generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { Env, Variables } from '../types/bindings';

const auth = new Hono<{ Bindings: Env; Variables: Variables }>();
const ACCESS_TOKEN_TTL_SECONDS = 60 * 60;
const REFRESH_TOKEN_TTL_SECONDS = 365 * 24 * 60 * 60;
const LOCAL_WEB_URL = 'http://localhost:5173';

const signupSchema = z.object({
  email: z.email(),
  password: z.string().min(1),
  name: z.string().min(1).optional(),
});

const loginSchema = z.object({
  email: z.email(),
  password: z.string().min(1),
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
  password: z.string().min(1),
  e2ee_key: z.base64().optional(),
  pub_key: z.base64().optional(),
  priv_key: z.base64().optional(),
  partner_access_keys: z
    .array(
      z.object({
        partnership_id: z.string().min(1),
        e2ee_key: z.base64(),
      }),
    )
    .optional(),
});

const updateUserSchema = z
  .object({
    email: z.email().optional(),
    name: z.string().min(1).optional(),
    e2ee_key: z.base64().optional(),
    pub_key: z.base64().optional(),
    priv_key: z.base64().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });

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

auth.post('/signup', validateZ('json', signupSchema), async (c) => {
  const { email, password, name } = c.req.valid('json');
  const existingUser = await findUserByEmail(c.env.DB, email);

  if (existingUser) {
    return c.json({ error: 'User already exists' }, 409);
  }

  const userId = uuidv4();
  const passwordHash = await hashPassword(password);

  await createUser(c.env.DB, { id: userId, email, passwordHash, name });
  await sendVerificationEmail(c, { id: userId, email, name });
  const accessToken = await createSession(c, userId);

  return c.json(
    {
      user: { id: userId, email, email_verified: false, ...(name ? { name } : {}) },
      access_token: accessToken,
    },
    201,
  );
});

auth.post('/login', validateZ('json', loginSchema), async (c) => {
  const { email, password } = c.req.valid('json');
  const user = await findUserByEmail(c.env.DB, email);

  if (!user || !(await verifyPassword(password, user.password_hash))) {
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
    ...(user.e2ee_key ? { e2ee_key: encodeBase64(user.e2ee_key) } : {}),
    ...(user.pub_key ? { pub_key: encodeBase64(user.pub_key) } : {}),
    ...(user.priv_key ? { priv_key: encodeBase64(user.priv_key) } : {}),
  });
});

auth.patch('/user', authenticate('access'), validateZ('json', updateUserSchema), async (c) => {
  const userId = c.get('sub');
  const { email, name, e2ee_key, pub_key, priv_key } = c.req.valid('json');
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

  await updateUser(c.env.DB, userId, {
    ...(emailChanged
      ? { email: normalizedEmail, email_verified: false, email_bounced_at: null }
      : {}),
    name,
    e2ee_key: e2ee_key ? decodeBase64(e2ee_key) : undefined,
    pub_key: pub_key ? decodeBase64(pub_key) : undefined,
    priv_key: priv_key ? decodeBase64(priv_key) : undefined,
  });

  return c.json({ ok: true });
});

auth.post('/verify-email', validateZ('json', verifyEmailSchema), async (c) => {
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

auth.post('/verify-email/request', authenticate('access'), async (c) => {
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

auth.post('/password-reset/request', validateZ('json', passwordResetRequestSchema), async (c) => {
  const { email } = c.req.valid('json');
  const user = await findUserByEmail(c.env.DB, email);

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
    user_id: record.user_id,
    key_rotation_required: Boolean(user.e2ee_key || user.pub_key || user.priv_key),
    partner_access_targets: (await listPartnerAccessTargetsForOwner(c.env.DB, record.user_id)).map(
      (target) => ({
        partnership_id: target.id,
        partner_email: target.partner_email!,
        ...(target.partner_pub_key
          ? { partner_pub_key: encodeBase64(target.partner_pub_key) }
          : {}),
      }),
    ),
  });
});

auth.post('/password-reset', validateZ('json', passwordResetSchema), async (c) => {
  const { token, password, e2ee_key, pub_key, priv_key, partner_access_keys } = c.req.valid('json');
  const record = await getValidTokenRecord(c.env.DB, token, 'password_reset');

  if (!record) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  const user = await findUserById(c.env.DB, record.user_id);
  if (!user) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  const keyRotationRequired = Boolean(user.e2ee_key || user.pub_key || user.priv_key);
  if (keyRotationRequired && (!e2ee_key || !pub_key || !priv_key)) {
    return c.json(
      { error: 'New encrypted key material must be generated during password reset' },
      400,
    );
  }

  await updateUser(c.env.DB, record.user_id, {
    password_hash: await hashPassword(password),
    ...(e2ee_key ? { e2ee_key: decodeBase64(e2ee_key) } : {}),
    ...(pub_key ? { pub_key: decodeBase64(pub_key) } : {}),
    ...(priv_key ? { priv_key: decodeBase64(priv_key) } : {}),
  });
  if (keyRotationRequired) {
    await clearPartnerAccessKeysForUser(c.env.DB, record.user_id);
    if (partner_access_keys?.length) {
      await updatePartnerAccessKeys(
        c.env.DB,
        record.user_id,
        partner_access_keys.map((key) => ({
          partnership_id: key.partnership_id,
          e2ee_key: decodeBase64(key.e2ee_key),
        })),
      );
    }
  }
  await consumeEmailToken(c.env.DB, record.id, Date.now());
  await invalidateEmailTokens(c.env.DB, record.user_id, 'password_reset');

  return c.json({ ok: true });
});

export default auth;
