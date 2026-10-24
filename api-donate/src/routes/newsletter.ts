import { Hono } from 'hono';
import { z } from 'zod';
import {
  confirmSubscriber,
  isConfirmationValid,
  releaseConfirmation,
  requestSubscription,
  unsubscribeSubscriber,
} from '../lib/db';
import { confirmationEmail, escapeHtml } from '../lib/newsletter-email';
import { signNewsletterToken, verifyNewsletterToken } from '../lib/newsletter-token';
import { sendSesEmail } from '../lib/ses';
import { verifyTurnstile } from '../lib/turnstile';
import { Env, Variables } from '../types/bindings';

const newsletter = new Hono<{ Bindings: Env; Variables: Variables }>();

const DEFAULT_DAILY_CONFIRMATION_CAP = 200;

// The landing form posts here as a plain HTML form, so the body is form-encoded.
const newsletterSchema = z.object({ email: z.string().trim().toLowerCase().pipe(z.email()) });
const tokenSchema = z.object({ token: z.string().max(512) });

/**
 * Sends the double opt-in email. Without AWS credentials the link is only logged in local dev;
 * anywhere else that is a send failure, and nothing about the address or link is logged. If
 * sending fails, the claim on this address is given back so the visitor can try again at once
 * instead of waiting out the hourly throttle.
 */
async function sendConfirmation(
  env: Env,
  origin: string,
  email: string,
  sentAt: number,
): Promise<void> {
  try {
    const token = await signNewsletterToken(env.NEWSLETTER_TOKEN_SECRET, 'confirm', email, sentAt);
    const content = confirmationEmail(`${origin}/newsletter/confirm?token=${token}`);

    if (!env.AWS_ACCESS_KEY_ID || !env.AWS_SECRET_ACCESS_KEY) {
      if (new URL(env.LANDING_URL).hostname !== 'localhost') {
        throw new Error('AWS credentials are not set');
      }
      console.log(
        `[newsletter] AWS credentials not set; confirmation for ${email}:\n${content.text}`,
      );
      return;
    }
    await sendSesEmail(
      {
        region: env.AWS_SES_REGION,
        accessKeyId: env.AWS_ACCESS_KEY_ID,
        secretAccessKey: env.AWS_SECRET_ACCESS_KEY,
      },
      {
        from: env.NEWSLETTER_FROM_EMAIL,
        to: email,
        configurationSet: env.NEWSLETTER_SES_CONFIG_SET,
        replyTo: env.NEWSLETTER_REPLY_TO,
        ...content,
      },
    );
  } catch (err) {
    console.error('[newsletter] failed to send confirmation', err);
    try {
      await releaseConfirmation(env.DB, email, sentAt);
    } catch (releaseErr) {
      console.error('[newsletter] failed to release confirmation claim', releaseErr);
    }
  }
}

function page(title: string, body: string, status: 200 | 400 = 200) {
  return new Response(
    `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${escapeHtml(title)}</title></head><body style="font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem;line-height:1.5"><h1>${escapeHtml(title)}</h1>${body}</body></html>`,
    { status, headers: { 'Content-Type': 'text/html; charset=utf-8' } },
  );
}

const invalidLink = (what: string) =>
  page('Link not valid', `<p>This ${what} link is not valid.</p>`, 400);

/**
 * Landing page newsletter signup. Records where the subscriber is (Cloudflare geoip) and emails
 * them a confirmation link. They only receive newsletters once they click it.
 *
 * This is an unauthenticated form, and every confirmation email counts against the SES account
 * that also sends account email, so it is guarded three ways: a per-IP rate limit, a Turnstile
 * check, and a daily cap on confirmation emails across all addresses. Every failure sends the
 * visitor back to the landing site, since this is a plain navigational form post.
 */
newsletter.post('/newsletter', async (c) => {
  const landing = c.env.LANDING_URL;
  const fail = () => c.redirect(`${landing}/newsletter/error`, 303);

  const body = await c.req.parseBody();
  const parsed = newsletterSchema.safeParse(body);
  if (!parsed.success) return c.redirect(`${landing}/#newsletter`, 303);
  const { email } = parsed.data;

  const ip = c.req.header('CF-Connecting-IP');
  if (c.env.NEWSLETTER_RATE_LIMITER) {
    const { success } = await c.env.NEWSLETTER_RATE_LIMITER.limit({ key: `ip:${ip ?? 'unknown'}` });
    if (!success) return fail();
  }
  const human = await verifyTurnstile(
    c.env.TURNSTILE_SECRET_KEY,
    body['cf-turnstile-response'],
    ip,
    c.env.TURNSTILE_HOSTNAMES,
  );
  if (!human) return fail();

  const cf = c.req.raw.cf;
  const cap = Number(c.env.NEWSLETTER_DAILY_CONFIRMATION_CAP) || DEFAULT_DAILY_CONFIRMATION_CAP;
  let request;
  try {
    request = await requestSubscription(
      c.env.DB,
      {
        email,
        country: cf?.country as string | undefined,
        regionCode: cf?.regionCode as string | undefined,
        region: cf?.region as string | undefined,
      },
      cap,
    );
  } catch (err) {
    console.error('[newsletter] failed to record subscriber', err);
    return fail();
  }

  if (request.status === 'capped') {
    console.error('[newsletter] daily confirmation cap reached; not sending');
    return fail();
  }
  if (request.status === 'send') {
    c.executionCtx.waitUntil(
      sendConfirmation(c.env, new URL(c.req.url).origin, email, request.sentAt),
    );
  }

  // The same redirect for sent and skipped, so the form doesn't reveal who is already subscribed.
  return c.redirect(`${landing}/newsletter/check-email`, 303);
});

// Mail scanners (Outlook SafeLinks, Proofpoint) fetch links, so GET only shows a button and the
// state change is a POST. That keeps a scanner from confirming someone who never clicked.
newsletter.get('/newsletter/confirm', async (c) => {
  const parsed = tokenSchema.safeParse(c.req.query());
  if (!parsed.success) return invalidLink('confirmation');
  const { token } = parsed.data;

  const claim = await verifyNewsletterToken(c.env.NEWSLETTER_TOKEN_SECRET, 'confirm', token);
  if (!claim || !(await isConfirmationValid(c.env.DB, claim.email, claim.issuedAt))) {
    return invalidLink('confirmation');
  }

  return page(
    'Confirm subscription',
    `<p>Get Virtue Initiative project updates at ${escapeHtml(claim.email)}?</p><form method="post" action="/newsletter/confirm?token=${encodeURIComponent(token)}"><button type="submit">Confirm subscription</button></form>`,
  );
});

newsletter.post('/newsletter/confirm', async (c) => {
  const parsed = tokenSchema.safeParse(c.req.query());
  if (!parsed.success) return invalidLink('confirmation');

  const claim = await verifyNewsletterToken(
    c.env.NEWSLETTER_TOKEN_SECRET,
    'confirm',
    parsed.data.token,
  );
  if (!claim || !(await confirmSubscriber(c.env.DB, claim.email, claim.issuedAt))) {
    return invalidLink('confirmation');
  }
  return c.redirect(`${c.env.LANDING_URL}/newsletter/confirmed`, 303);
});

newsletter.get('/newsletter/unsubscribe', async (c) => {
  const parsed = tokenSchema.safeParse(c.req.query());
  if (!parsed.success) return invalidLink('unsubscribe');
  const { token } = parsed.data;

  const claim = await verifyNewsletterToken(c.env.NEWSLETTER_TOKEN_SECRET, 'unsubscribe', token);
  if (!claim) return invalidLink('unsubscribe');

  return page(
    'Unsubscribe',
    `<p>Stop sending newsletters to ${escapeHtml(claim.email)}?</p><form method="post" action="/newsletter/unsubscribe?token=${encodeURIComponent(token)}"><button type="submit">Unsubscribe</button></form>`,
  );
});

// RFC 8058 one-click clients POST here directly.
newsletter.post('/newsletter/unsubscribe', async (c) => {
  const parsed = tokenSchema.safeParse(c.req.query());
  if (!parsed.success) return invalidLink('unsubscribe');

  const claim = await verifyNewsletterToken(
    c.env.NEWSLETTER_TOKEN_SECRET,
    'unsubscribe',
    parsed.data.token,
  );
  if (!claim) return invalidLink('unsubscribe');

  await unsubscribeSubscriber(c.env.DB, claim.email);
  return page(
    'Unsubscribed',
    `<p>${escapeHtml(claim.email)} will no longer get our newsletter.</p>`,
  );
});

export default newsletter;
