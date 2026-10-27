import { Hono } from 'hono';
import { z } from 'zod';
import { sendEmail } from '../lib/email';
import { renderNewsletterConfirmationTemplate } from '../lib/email/templates';
import {
  claimNewsletterConfirmation,
  confirmNewsletterSignup,
  requestLocation,
  unsubscribeNewsletterToken,
} from '../lib/newsletter';
import { verifyTurnstile } from '../lib/turnstile';
import { rateLimitByIp } from '../middleware/rate-limit';
import { Env, Variables } from '../types/bindings';

// These are plain HTML form posts from the landing site, so they answer with a redirect back to
// it instead of JSON.
const newsletter = new Hono<{ Bindings: Env; Variables: Variables }>();

async function formFields(req: Request): Promise<Record<string, string>> {
  const form = await req.formData().catch(() => null);
  const fields: Record<string, string> = {};
  form?.forEach((value, key) => {
    if (typeof value === 'string') fields[key] = value;
  });
  return fields;
}

/** POST /newsletter - the landing site's newsletter form (API-058). */
newsletter.post('/newsletter', rateLimitByIp(), async (c) => {
  const landing = c.env.LANDING_URL;
  const form = await formFields(c.req.raw);
  const email = z.email().safeParse((form.email ?? '').trim());

  const verified = await verifyTurnstile(
    c.env.TURNSTILE_SECRET_KEY,
    form['cf-turnstile-response'],
    c.req.header('CF-Connecting-IP'),
  );
  if (!email.success || !verified) {
    return c.redirect(`${landing}/newsletter/error`, 303);
  }

  const address = email.data.toLowerCase();
  const token = await claimNewsletterConfirmation(c.env, address, requestLocation(c.req.raw));
  if (token) {
    const confirmUrl = `${landing}/newsletter/confirm?token=${token}`;
    const content = renderNewsletterConfirmationTemplate({
      appName: c.env.APP_NAME,
      appUrl: c.env.APP_URL,
      confirmUrl,
    });
    // sendEmail skips an address with a permanent bounce (API-056).
    await sendEmail({
      env: c.env,
      db: c.env.DB,
      kind: 'newsletter_confirmation',
      recipient: address,
      allowUnverified: true,
      metadata: { purpose: 'newsletter_confirmation', confirmUrl },
      ...content,
    }).catch((err: unknown) => console.error('[newsletter] confirmation email failed', err));
  }
  return c.redirect(`${landing}/newsletter/check-email`, 303);
});

/** POST /newsletter/confirm - the button on the landing site's confirm page (API-059). */
newsletter.post('/newsletter/confirm', async (c) => {
  const { token } = await formFields(c.req.raw);
  const confirmed = token ? await confirmNewsletterSignup(c.env, token) : false;
  return c.redirect(`${c.env.LANDING_URL}/newsletter/${confirmed ? 'subscribed' : 'error'}`, 303);
});

/**
 * POST /newsletter/unsubscribe - the landing site's unsubscribe button, and RFC 8058 one-click
 * unsubscribes from mail clients, which post `List-Unsubscribe=One-Click` (API-060).
 */
newsletter.post('/newsletter/unsubscribe', async (c) => {
  const form = await formFields(c.req.raw);
  const token = c.req.query('token') || form.token;
  const unsubscribed = token ? await unsubscribeNewsletterToken(c.env, token) : false;

  if (form['List-Unsubscribe'] === 'One-Click') {
    return unsubscribed ? c.json({ ok: true }) : c.json({ error: 'Unknown token' }, 400);
  }
  return c.redirect(
    `${c.env.LANDING_URL}/newsletter/${unsubscribed ? 'unsubscribed' : 'error'}`,
    303,
  );
});

export default newsletter;
