import { Hono } from 'hono';
import { z } from 'zod';
import { suppressSubscribers } from '../lib/db';
import { isSnsUrl, parseTopicArnAllowlist, snsMessageSchema, verifySnsSignature } from '../lib/sns';
import { Env, Variables } from '../types/bindings';

const newsletterEvents = new Hono<{ Bindings: Env; Variables: Variables }>();

type SesEvent = {
  eventType?: string;
  bounce?: { bounceType?: string; bouncedRecipients?: Array<{ emailAddress?: string }> };
  complaint?: { complainedRecipients?: Array<{ emailAddress?: string }> };
};

/**
 * Addresses to stop mailing: hard bounces and complaints. A transient bounce may deliver later.
 * Returns null for a message that isn't an SES event, such as a manual test publish.
 */
function suppressedEmails(message: string): string[] | null {
  let parsed: SesEvent;
  try {
    parsed = JSON.parse(message) as SesEvent;
  } catch {
    return null;
  }
  if (typeof parsed !== 'object' || parsed === null) return null;

  const recipients =
    parsed.eventType === 'Bounce' && parsed.bounce?.bounceType === 'Permanent'
      ? (parsed.bounce.bouncedRecipients ?? [])
      : parsed.eventType === 'Complaint'
        ? (parsed.complaint?.complainedRecipients ?? [])
        : [];

  return Array.from(
    new Set(
      recipients.map((r) => r.emailAddress?.trim().toLowerCase()).filter((e): e is string => !!e),
    ),
  );
}

/**
 * Receives SES bounce and complaint events for newsletter mail, published to SNS by the
 * `newsletter` configuration set. Same checks as the main API's /email/sns (API-041): an
 * allowlisted topic and a message SNS itself signed, before anything is fetched or acted on.
 */
newsletterEvents.post('/newsletter/sns', async (c) => {
  const parsed = snsMessageSchema.safeParse(await c.req.json().catch(() => null));
  if (!parsed.success) {
    return c.json({ error: 'Invalid request data', details: z.treeifyError(parsed.error) }, 400);
  }
  const body = parsed.data;

  if (!parseTopicArnAllowlist(c.env.SNS_TOPIC_ARNS).includes(body.TopicArn)) {
    return c.json({ error: 'Unknown SNS topic' }, 403);
  }
  if (!(await verifySnsSignature(body))) {
    return c.json({ error: 'Invalid SNS signature' }, 403);
  }

  if (body.Type === 'SubscriptionConfirmation') {
    if (!body.SubscribeURL || !isSnsUrl(body.SubscribeURL)) {
      return c.json({ error: 'Invalid SubscribeURL' }, 403);
    }
    await fetch(body.SubscribeURL, { redirect: 'manual' });
    return c.json({ ok: true, subscribed: true });
  }

  if (body.Type !== 'Notification') {
    return c.json({ ok: true });
  }

  const emails = suppressedEmails(body.Message);
  if (emails === null) {
    // 200 so SNS doesn't retry something we can never parse.
    console.warn('[newsletter] ignoring SNS message that is not an SES event');
    return c.json({ ok: true });
  }
  if (emails.length === 0) {
    // A transient bounce is expected. Anything else is a shape we don't act on, so leave a trace.
    console.log('[newsletter] SES event suppressed no one');
    return c.json({ ok: true, updated: 0 });
  }

  await suppressSubscribers(c.env.DB, emails);
  return c.json({ ok: true, updated: emails.length });
});

export default newsletterEvents;
