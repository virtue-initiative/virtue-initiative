import { Hono } from 'hono';
import { z } from 'zod';
import { addNewsletterSignup, requestLocation } from '../lib/newsletter';
import { verifyTurnstile } from '../lib/turnstile';
import { rateLimitByIp } from '../middleware/rate-limit';
import { Env, Variables } from '../types/bindings';

const newsletter = new Hono<{ Bindings: Env; Variables: Variables }>();

/**
 * POST /newsletter - the landing site's newsletter form (API-058). A plain HTML form post, so it
 * answers with a redirect back to the landing site instead of JSON.
 */
newsletter.post('/newsletter', rateLimitByIp(), async (c) => {
  const landing = c.env.LANDING_URL;
  const form = await c.req.parseBody().catch(() => ({}) as Record<string, unknown>);
  const email = z.email().safeParse(String(form.email ?? '').trim());

  const verified = await verifyTurnstile(
    c.env.TURNSTILE_SECRET_KEY,
    form['cf-turnstile-response'],
    c.req.header('CF-Connecting-IP'),
  );
  if (!email.success || !verified) {
    return c.redirect(`${landing}/newsletter/error`, 303);
  }

  await addNewsletterSignup(c.env, email.data.toLowerCase(), requestLocation(c.req.raw));
  return c.redirect(`${landing}/newsletter/subscribed`, 303);
});

export default newsletter;
