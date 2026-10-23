import { zValidator } from '@hono/zod-validator';
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
newsletter.post('/newsletter', zValidator('form', newsletterSchema), async (c) => {
  const { email } = c.req.valid('form');
  const cf = c.req.raw.cf;

  await recordSubscriber(c.env.DB, {
    email,
    country: cf?.country as string | undefined,
    regionCode: cf?.regionCode as string | undefined,
    region: cf?.region as string | undefined,
    source: 'landing',
  });

  // Best-effort for the same reason as the signup flow: the region is already saved.
  c.executionCtx.waitUntil(subscribeToButtondown(c.env, email, c.req.header('CF-Connecting-IP')));

  return c.redirect(`${c.env.LANDING_URL}/newsletter/check-email`, 303);
});

export default newsletter;
