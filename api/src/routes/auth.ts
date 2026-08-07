import { Context, Hono } from 'hono';
import { getCookie, deleteCookie, setCookie } from 'hono/cookie';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import { authenticateWebSession } from '../middleware/auth';
import { validateZ } from '../middleware/validation';
import {
  createEmailToken,
  createSessionRecord,
  createUser,
  deleteUserById,
  deleteSessionByRefreshTokenHash,
  findEmailTokenByHash,
  findUserByEmail,
  findUserById,
  invalidateEmailTokens,
  updateUser,
  consumeEmailToken,
} from '../lib/db';
import {
  renderAccountExistsTemplate,
  renderEmailInUseTemplate,
  renderEmailVerificationTemplate,
  renderPasswordResetTemplate,
} from '../lib/email/templates';
import { sendEmail } from '../lib/email';
import { decodeBase64, encodeBase64 } from '../lib/encoding';
import { EMAIL_VERIFICATION_TTL_MS, PASSWORD_RESET_TTL_MS } from '../lib/email-domain';
import {
  signupRequestSchema,
  signupSchema,
  loginMaterialQuerySchema,
  loginSchema,
  verifyEmailSchema,
  passwordResetRequestSchema,
  passwordResetValidateSchema,
  passwordResetSchema,
  updateUserSchema,
  deleteUserSchema,
  type SignupResponse,
  type EmailVerifyResponse,
  type UpdateUserResponse,
} from '../../../shared-web/types';
import {
  CURRENT_HASH_PARAMS,
  HASH_PARAMS_VERSION,
  generatePasswordSalt,
  hashPasswordAuth,
} from '../lib/password';
import { assertTokenPurpose, generateOpaqueToken, hashOpaqueToken } from '../lib/tokens';
import { Env, Variables } from '../types/bindings';
import { verifyUserCredentials } from '../lib/credentials';

const auth = new Hono<{ Bindings: Env; Variables: Variables }>();
const REFRESH_TOKEN_TTL_SECONDS = 365 * 24 * 60 * 60;
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

// Base64 decode + byte-length validation, layered on top of the wire-level
// `z.base64()` string schemas in shared-web/types.ts (which stay strings since
// web serializes these fields as JSON).
function base64Bytes(length: number, label: string) {
  return z.base64().transform((value, ctx) => {
    const decoded = decodeBase64(value);
    if (new Uint8Array(decoded).byteLength !== length) {
      ctx.addIssue({ code: 'custom', message: `${label} must be ${length} bytes` });
      return z.NEVER;
    }
    return decoded;
  });
}

function base64NonEmpty(label: string) {
  return z.base64().transform((value, ctx) => {
    const decoded = decodeBase64(value);
    if (new Uint8Array(decoded).byteLength === 0) {
      ctx.addIssue({ code: 'custom', message: `${label} must not be empty` });
      return z.NEVER;
    }
    return decoded;
  });
}

const keyMaterialSchema = z.object({
  password_auth: base64Bytes(32, 'password_auth'),
  password_salt: base64Bytes(CURRENT_HASH_PARAMS.salt_length, 'password_salt'),
  pub_key: base64Bytes(32, 'pub_key'),
  encrypted_priv_key: base64NonEmpty('encrypted_priv_key'),
});

const updateKeyMaterialSchema = z.object({
  pub_key: base64Bytes(32, 'pub_key').optional(),
  encrypted_priv_key: base64NonEmpty('encrypted_priv_key').optional(),
});

function invalidRequestData(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  error: z.ZodError,
) {
  return c.json({ error: 'Invalid request data', details: z.treeifyError(error) }, 400);
}

async function createSession(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  userId: string,
): Promise<string> {
  const refreshToken = generateOpaqueToken('web_session');
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

  return refreshToken;
}

async function issueEmailToken(
  db: D1Database,
  user: { id: string; email: string },
  purpose: 'email_change' | 'password_reset',
  ttlMs: number,
) {
  await invalidateEmailTokens(db, user.id, purpose);
  const token = generateOpaqueToken(purpose);
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
  token: string,
) {
  const verifyUrl = `${c.env.APP_URL}/verify-email?token=${encodeURIComponent(token)}`;
  const email = renderEmailVerificationTemplate({
    appName: c.env.APP_NAME,
    appUrl: c.env.APP_URL,
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
    metadata: { purpose: 'email_change', verifyUrl },
  });
}

async function sendSignupConfirmationEmail(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  recipient: { email: string; name?: string | null },
  token: string,
  options?: { to?: string },
) {
  const params = new URLSearchParams({ signup_token: token });
  if (options?.to) {
    params.set('to', options.to);
  }
  const verifyUrl = `${c.env.APP_URL}/signup?${params.toString()}`;
  const email = renderEmailVerificationTemplate({
    appName: c.env.APP_NAME,
    appUrl: c.env.APP_URL,
    recipientName: recipient.name,
    verifyUrl,
  });

  await sendEmail({
    env: c.env,
    db: c.env.DB,
    kind: 'email_verification',
    recipient: recipient.email,
    subject: email.subject,
    text: email.text,
    html: email.html,
    metadata: { purpose: 'signup', verifyUrl },
  });
}

async function sendAccountExistsEmail(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  user: { id: string; email: string; name?: string | null },
) {
  const loginUrl = `${c.env.APP_URL}/login`;
  const forgotPasswordUrl = `${c.env.APP_URL}/forgot-password`;
  const email = renderAccountExistsTemplate({
    appName: c.env.APP_NAME,
    appUrl: c.env.APP_URL,
    recipientName: user.name,
    loginUrl,
    forgotPasswordUrl,
  });

  await sendEmail({
    env: c.env,
    db: c.env.DB,
    kind: 'account_exists_notice',
    recipient: user.email,
    subject: email.subject,
    text: email.text,
    html: email.html,
    related_user_id: user.id,
    metadata: { purpose: 'account_exists', loginUrl, forgotPasswordUrl },
  });
}

async function sendEmailInUseNotice(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  user: { id: string; email: string; name?: string | null },
) {
  const forgotPasswordUrl = `${c.env.APP_URL}/forgot-password`;
  const email = renderEmailInUseTemplate({
    appName: c.env.APP_NAME,
    appUrl: c.env.APP_URL,
    recipientName: user.name,
    forgotPasswordUrl,
  });

  await sendEmail({
    env: c.env,
    db: c.env.DB,
    kind: 'email_in_use_notice',
    recipient: user.email,
    subject: email.subject,
    text: email.text,
    html: email.html,
    related_user_id: user.id,
    metadata: { purpose: 'email_in_use', forgotPasswordUrl },
  });
}

async function sendPasswordResetEmail(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  user: { id: string; email: string; name?: string | null },
) {
  const token = await issueEmailToken(c.env.DB, user, 'password_reset', PASSWORD_RESET_TTL_MS);
  const resetUrl = `${c.env.APP_URL}/forgot-password?token=${encodeURIComponent(token)}`;
  const email = renderPasswordResetTemplate({
    appName: c.env.APP_NAME,
    appUrl: c.env.APP_URL,
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
  purpose: 'email_change' | 'password_reset' | 'signup',
) {
  try {
    assertTokenPurpose(rawToken, purpose);
  } catch {
    return null;
  }

  const token = await findEmailTokenByHash(db, hashOpaqueToken(rawToken), purpose);
  if (!token || token.consumed_at || token.expires_at < Date.now()) {
    return null;
  }
  if (token.purpose !== 'signup' && !token.user_id) {
    return null;
  }
  return token;
}

auth.get('/user/login-material', validateZ('query', loginMaterialQuerySchema), async (c) => {
  const { email } = c.req.valid('query');

  if (!email) {
    return c.json({ params: buildHashParamsResponse() });
  }

  const user = await findUserByEmail(c.env.DB, email.trim().toLowerCase());

  return c.json({
    password_salt: encodeBase64(user?.password_salt ?? generatePasswordSalt()),
    params: buildHashParamsResponse(),
  });
});

auth.post('/signup-request', validateZ('json', signupRequestSchema), async (c) => {
  const { email, to } = c.req.valid('json');
  const normalizedEmail = email.trim().toLowerCase();
  const existingUser = await findUserByEmail(c.env.DB, normalizedEmail);

  if (existingUser) {
    await sendAccountExistsEmail(c, existingUser);
    return c.json({ ok: true });
  }

  const token = generateOpaqueToken('signup');
  const now = Date.now();

  await createEmailToken(c.env.DB, {
    id: uuidv4(),
    user_id: null,
    email: normalizedEmail,
    purpose: 'signup',
    token_hash: hashOpaqueToken(token),
    expires_at: now + EMAIL_VERIFICATION_TTL_MS,
    created_at: now,
  });

  await sendSignupConfirmationEmail(c, { email: normalizedEmail }, token, {
    to: to ?? undefined,
  });

  return c.json({ ok: true });
});

auth.post('/signup', validateZ('json', signupSchema), async (c) => {
  const { verification_token, password_auth, password_salt, pub_key, encrypted_priv_key, name } =
    c.req.valid('json');

  const record = await getValidTokenRecord(c.env.DB, verification_token, 'signup');
  if (!record) {
    return c.json({ error: 'Invalid or expired verification token' }, 400);
  }

  const normalizedEmail = record.email;
  const existingUser = await findUserByEmail(c.env.DB, normalizedEmail);

  if (existingUser) {
    // Someone else already claimed this email between /signup-request and now.
    // /signup-request already notified the account owner, so just burn the
    // token and report the same generic failure — no fresh leak here.
    await consumeEmailToken(c.env.DB, record, Date.now());
    return c.json({ error: 'Invalid or expired verification token' }, 400);
  }

  const decoded = keyMaterialSchema.safeParse({
    password_auth,
    password_salt,
    pub_key,
    encrypted_priv_key,
  });
  if (!decoded.success) {
    return invalidRequestData(c, decoded.error);
  }

  const userId = uuidv4();
  const passwordHash = await hashPasswordAuth(decoded.data.password_auth);

  await createUser(c.env.DB, {
    id: userId,
    email: normalizedEmail,
    passwordHash,
    passwordSalt: decoded.data.password_salt,
    passwordParamsVersion: HASH_PARAMS_VERSION,
    pub_key: decoded.data.pub_key,
    encrypted_priv_key: decoded.data.encrypted_priv_key,
    name,
  });

  await updateUser(c.env.DB, userId, { email_verified: true });
  await consumeEmailToken(c.env.DB, record, Date.now());

  await createSession(c, userId);

  return c.json<SignupResponse>(
    {
      user: {
        id: userId,
        email: normalizedEmail,
        email_verified: true,
        ...(name ? { name } : {}),
      },
    },
    201,
  );
});

auth.post('/login', validateZ('json', loginSchema), async (c) => {
  const { email, password_auth, timezone } = c.req.valid('json');
  const result = await verifyUserCredentials(c.env.DB, email, password_auth);

  if (result.status === 'invalid') {
    return c.json({ error: 'Invalid email or password' }, 401);
  }

  if (result.status === 'unverified') {
    return c.json({ error: 'Please verify your email before logging in.' }, 403);
  }

  const { user } = result;

  if (timezone) {
    await updateUser(c.env.DB, user.id, { settings: { timezone } });
  }

  await createSession(c, user.id);
  return c.json({ ok: true });
});

auth.post('/logout', async (c) => {
  const refreshToken = getCookie(c, 'refresh_token');
  if (refreshToken) {
    try {
      assertTokenPurpose(refreshToken, 'web_session');
      await deleteSessionByRefreshTokenHash(c.env.DB, hashOpaqueToken(refreshToken), 'web');
    } catch {
      // Malformed or foreign-purpose token — nothing to delete.
    }
  }
  deleteCookie(c, 'refresh_token', { path: '/' });
  return c.body(null, 204);
});

auth.get('/user', authenticateWebSession(), async (c) => {
  const user = await findUserById(c.env.DB, c.get('sub'));

  if (!user) {
    return c.json({ error: 'User account not found' }, 404);
  }

  return c.json({
    id: user.id,
    email: user.email,
    email_verified: user.email_verified === 1,
    email_bounced_at: user.email_bounced_at,
    settings: user.settings,
    ...(user.name ? { name: user.name } : {}),
    ...(user.pub_key ? { pub_key: encodeBase64(user.pub_key) } : {}),
    ...(user.encrypted_priv_key
      ? { encrypted_priv_key: encodeBase64(user.encrypted_priv_key) }
      : {}),
  });
});

auth.patch('/user', authenticateWebSession(), validateZ('json', updateUserSchema), async (c) => {
  const userId = c.get('sub');
  const { email, name, settings, pub_key, encrypted_priv_key } = c.req.valid('json');
  const normalizedEmail = email?.trim().toLowerCase();
  const user = await findUserById(c.env.DB, userId);

  if (!user) {
    return c.json({ error: 'User account not found' }, 404);
  }

  const decoded = updateKeyMaterialSchema.safeParse({ pub_key, encrypted_priv_key });
  if (!decoded.success) {
    return invalidRequestData(c, decoded.error);
  }

  await updateUser(c.env.DB, userId, {
    name,
    settings,
    pub_key: decoded.data.pub_key,
    encrypted_priv_key: decoded.data.encrypted_priv_key,
  });

  const emailChanged = Boolean(normalizedEmail && normalizedEmail !== user.email);
  if (emailChanged) {
    const existingUser = await findUserByEmail(c.env.DB, normalizedEmail!);
    if (existingUser && existingUser.id !== userId) {
      // Don't issue a real verification token — just let the actual owner
      // know, and tell the requester the same thing a successful request
      // would say (mirrors POST /password-reset's generic 204 pattern).
      await sendEmailInUseNotice(c, existingUser);
    } else {
      const verificationToken = await issueEmailToken(
        c.env.DB,
        { id: userId, email: normalizedEmail! },
        'email_change',
        EMAIL_VERIFICATION_TTL_MS,
      );
      await sendVerificationEmail(
        c,
        {
          id: userId,
          email: normalizedEmail!,
          name: name ?? user.name,
        },
        verificationToken,
      );
    }
  }

  return c.json<UpdateUserResponse>({
    ok: true,
    ...(emailChanged
      ? {
          email_verification_required: true,
          pending_email: normalizedEmail,
        }
      : {}),
  });
});

auth.delete('/user', authenticateWebSession(), validateZ('json', deleteUserSchema), async (c) => {
  const userId = c.get('sub');
  const { confirm_email } = c.req.valid('json');
  const user = await findUserById(c.env.DB, userId);

  if (!user) {
    return c.json({ error: 'User account not found' }, 404);
  }

  if (confirm_email.trim().toLowerCase() !== user.email) {
    return c.json({ error: 'Confirmation email does not match your account email' }, 400);
  }

  // R2 batch blobs age out via a bucket lifecycle rule independent of D1 row
  // lifetime, so there's nothing to clean up here beyond the cascading D1 delete.
  await deleteUserById(c.env.DB, userId);

  deleteCookie(c, 'refresh_token', { path: '/' });
  return c.body(null, 204);
});

auth.post('/email-verification/validate', validateZ('json', verifyEmailSchema), async (c) => {
  const { token } = c.req.valid('json');
  const record = await getValidTokenRecord(c.env.DB, token, 'email_change');

  if (!record || !record.user_id) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  const userId = record.user_id;

  const existingUser = await findUserByEmail(c.env.DB, record.email);
  if (existingUser && existingUser.id !== userId) {
    await consumeEmailToken(c.env.DB, record, Date.now());
    await sendEmailInUseNotice(c, existingUser);
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  await updateUser(c.env.DB, userId, {
    email: record.email,
    email_verified: true,
    email_bounced_at: null,
  });
  await consumeEmailToken(c.env.DB, record, Date.now());

  await createSession(c, userId);

  return c.json<EmailVerifyResponse>({
    ok: true,
    email: record.email,
    purpose: 'email_change',
  });
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

  if (!record || !record.user_id) {
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
  const { token, password_auth, password_salt, pub_key, encrypted_priv_key } = c.req.valid('json');
  const record = await getValidTokenRecord(c.env.DB, token, 'password_reset');

  if (!record || !record.user_id) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  const user = await findUserById(c.env.DB, record.user_id);
  if (!user) {
    return c.json({ error: 'Invalid or expired token' }, 400);
  }

  const decoded = keyMaterialSchema.safeParse({
    password_auth,
    password_salt,
    pub_key,
    encrypted_priv_key,
  });
  if (!decoded.success) {
    return invalidRequestData(c, decoded.error);
  }

  await updateUser(c.env.DB, record.user_id, {
    password_hash: await hashPasswordAuth(decoded.data.password_auth),
    password_salt: decoded.data.password_salt,
    password_params_version: HASH_PARAMS_VERSION,
    pub_key: decoded.data.pub_key,
    encrypted_priv_key: decoded.data.encrypted_priv_key,
  });
  await consumeEmailToken(c.env.DB, record, Date.now());

  return c.json({ ok: true });
});

export default auth;
