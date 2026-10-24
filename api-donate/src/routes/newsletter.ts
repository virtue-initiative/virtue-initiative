import { zValidator } from '@hono/zod-validator';
import { Hono } from 'hono';
import { z } from 'zod';
import { confirmSubscriber, requestSubscription, unsubscribeSubscriber } from '../lib/db';
import { confirmationEmail, escapeHtml } from '../lib/newsletter-email';
import { signNewsletterToken, verifyNewsletterToken } from '../lib/newsletter-token';
import { sendSesEmail } from '../lib/ses';
import { Env, Variables } from '../types/bindings';

const newsletter = new Hono<{ Bindings: Env; Variables: Variables }>();

// The landing form posts here as a plain HTML form, so the body is form-encoded.
const newsletterSchema = z.object({ email: z.string().trim().toLowerCase().pipe(z.email()) });
const tokenSchema = z.object({ token: z.string().max(512) });

/**
 * Sends the double opt-in email. Without AWS credentials (local dev) the link is only logged.
 * Best-effort: the signup is already saved, and the form can be resubmitted after an hour.
 */
async function sendConfirmation(env: Env, origin: string, email: string): Promise<void> {
  try {
    const token = await signNewsletterToken(env.NEWSLETTER_TOKEN_SECRET, 'confirm', email);
    const content = confirmationEmail(`${origin}/newsletter/confirm?token=${token}`);

    if (!env.AWS_ACCESS_KEY_ID || !env.AWS_SECRET_ACCESS_KEY) {
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
        ...content,
      },
    );
  } catch (err) {
    console.error('[newsletter] failed to send confirmation', err);
  }
}

function page(title: string, body: string, status: 200 | 400 = 200) {
  return new Response(
    `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${escapeHtml(title)}</title></head><body style="font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem;line-height:1.5"><h1>${escapeHtml(title)}</h1>${body}</body></html>`,
    { status, headers: { 'Content-Type': 'text/html; charset=utf-8' } },
  );
}

/**
 * Landing page newsletter signup. Records where the subscriber is (Cloudflare geoip) and emails
 * them a confirmation link. They only receive newsletters once they click it.
 */
newsletter.post('/newsletter', zValidator('form', newsletterSchema), async (c) => {
  const { email } = c.req.valid('form');
  const cf = c.req.raw.cf;

  const shouldConfirm = await requestSubscription(c.env.DB, {
    email,
    country: cf?.country as string | undefined,
    regionCode: cf?.regionCode as string | undefined,
    region: cf?.region as string | undefined,
  });

  if (shouldConfirm) {
    c.executionCtx.waitUntil(sendConfirmation(c.env, new URL(c.req.url).origin, email));
  }

  // The same redirect either way, so the form doesn't reveal who is already subscribed.
  return c.redirect(`${c.env.LANDING_URL}/newsletter/check-email`, 303);
});

newsletter.get('/newsletter/confirm', zValidator('query', tokenSchema), async (c) => {
  const email = await verifyNewsletterToken(
    c.env.NEWSLETTER_TOKEN_SECRET,
    'confirm',
    c.req.valid('query').token,
  );
  if (!email || !(await confirmSubscriber(c.env.DB, email))) {
    return page('Link not valid', '<p>This confirmation link is not valid.</p>', 400);
  }
  return c.redirect(`${c.env.LANDING_URL}/newsletter/confirmed`, 303);
});

// Mail scanners fetch links, so GET only shows a button. RFC 8058 one-click clients POST directly.
newsletter.get('/newsletter/unsubscribe', zValidator('query', tokenSchema), async (c) => {
  const { token } = c.req.valid('query');
  const email = await verifyNewsletterToken(c.env.NEWSLETTER_TOKEN_SECRET, 'unsubscribe', token);
  if (!email) return page('Link not valid', '<p>This unsubscribe link is not valid.</p>', 400);

  return page(
    'Unsubscribe',
    `<p>Stop sending newsletters to ${escapeHtml(email)}?</p><form method="post" action="/newsletter/unsubscribe?token=${encodeURIComponent(token)}"><button type="submit">Unsubscribe</button></form>`,
  );
});

newsletter.post('/newsletter/unsubscribe', zValidator('query', tokenSchema), async (c) => {
  const email = await verifyNewsletterToken(
    c.env.NEWSLETTER_TOKEN_SECRET,
    'unsubscribe',
    c.req.valid('query').token,
  );
  if (!email) return page('Link not valid', '<p>This unsubscribe link is not valid.</p>', 400);

  await unsubscribeSubscriber(c.env.DB, email);
  return page('Unsubscribed', `<p>${escapeHtml(email)} will no longer get our newsletter.</p>`);
});

export default newsletter;
