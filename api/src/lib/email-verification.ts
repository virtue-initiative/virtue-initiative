import { Context } from 'hono';
import { sendEmail } from './email';
import { issueEmailToken } from './email-tokens';
import { EMAIL_VERIFICATION_TTL_MS } from './email-domain';
import { HELP_EMAIL, renderEmailVerificationTemplate } from './email/templates';
import { Env, Variables } from '../types/bindings';

export async function sendVerificationEmail(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  user: { id: string; email: string; name?: string | null },
  { id: tokenId, token }: { id: string; token: string },
  purpose: 'email_change' | 'email_verification' = 'email_change',
) {
  const verifyUrl = `${c.env.APP_URL}/verify-email?token=${encodeURIComponent(token)}`;
  const email = renderEmailVerificationTemplate({
    appName: c.env.APP_NAME,
    appUrl: c.env.APP_URL,
    recipientName: user.name,
    verifyUrl,
  });

  return sendEmail({
    env: c.env,
    db: c.env.DB,
    kind: 'email_verification',
    recipient: user.email,
    subject: email.subject,
    text: email.text,
    html: email.html,
    related_user_id: user.id,
    email_token_id: tokenId,
    metadata: { purpose, verifyUrl },
  });
}

/** API-016, API-035: the answer to correct credentials on an unverified account. */
export async function unverifiedLoginResponse(
  c: Context<{ Bindings: Env; Variables: Variables }>,
  user: { id: string; email: string; name?: string | null },
) {
  const verificationToken = await issueEmailToken(
    c.env.DB,
    user,
    'email_verification',
    EMAIL_VERIFICATION_TTL_MS,
  );
  const sent = await sendVerificationEmail(c, user, verificationToken, 'email_verification');
  if (sent.bounce_id) {
    return c.json(
      {
        error: `Email to your address bounced, so we could not send a verification email. Email ${HELP_EMAIL} for help.`,
        code: 'email_bounced',
      },
      403,
    );
  }
  return c.json(
    { error: 'Please verify your email before logging in. A verification email has been sent.' },
    403,
  );
}
