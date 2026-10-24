import { Hono } from 'hono';
import { z } from 'zod';
import { recordSubscriber } from '../lib/db';
import { subscribeToButtondown } from '../lib/buttondown';
import { Env, Variables } from '../types/bindings';

const newsletter = new Hono<{ Bindings: Env; Variables: Variables }>();

// The landing form posts here as a plain HTML form, so the body is form-encoded.
const newsletterSchema = z.object({ email: z.string().trim().toLowerCase().pipe(z.email()) });

/**
 * Landing page newsletter signup. Records where the subscriber is (Cloudflare geoip) and adds
 * them to Buttondown as unconfirmed, so Buttondown still sends its confirmation email.
 */
newsletter.post('/newsletter', async (c) => {
  // This is a plain navigational form post, so send a bad address back to the form rather than
  // showing the visitor a JSON error page on this worker's domain.
  const parsed = newsletterSchema.safeParse(await c.req.parseBody());
  if (!parsed.success) return c.redirect(`${c.env.LANDING_URL}/#newsletter`, 303);
  const { email } = parsed.data;
  const cf = c.req.raw.cf;

  // The Buttondown signup must still happen if the database write fails.
  try {
    await recordSubscriber(c.env.DB, {
      email,
      country: cf?.country as string | undefined,
      regionCode: cf?.regionCode as string | undefined,
      region: cf?.region as string | undefined,
      source: 'landing',
    });
  } catch (err) {
    console.error('[newsletter] failed to record subscriber region', err);
  }

  // Best-effort: failures are logged and never block the redirect.
  c.executionCtx.waitUntil(subscribeToButtondown(c.env, email, c.req.header('CF-Connecting-IP')));

  return c.redirect(`${c.env.LANDING_URL}/newsletter/check-email`, 303);
});

export default newsletter;
