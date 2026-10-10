import { Hono } from 'hono';
import { z } from 'zod';
import { markUsersEmailBouncedByEmails, markUsersUnverifiedByEmails } from '../lib/db';
import { isSnsUrl, parseTopicArnAllowlist, snsMessageSchema, verifySnsSignature } from '../lib/sns';
import { Env, Variables } from '../types/bindings';

const emailWebhooks = new Hono<{ Bindings: Env; Variables: Variables }>();

function extractComplaintOrBounceEmails(message: string) {
  const parsed = JSON.parse(message) as {
    eventType?: string;
    bounce?: { bouncedRecipients?: Array<{ emailAddress?: string }> };
    complaint?: { complainedRecipients?: Array<{ emailAddress?: string }> };
  };

  const bouncedEmails =
    parsed.eventType === 'Bounce'
      ? ((parsed.bounce?.bouncedRecipients ?? [])
          .map((recipient) => recipient.emailAddress?.trim().toLowerCase())
          .filter(Boolean) as string[])
      : [];

  const complaintEmails =
    parsed.eventType === 'Complaint'
      ? ((parsed.complaint?.complainedRecipients ?? [])
          .map((recipient) => recipient.emailAddress?.trim().toLowerCase())
          .filter(Boolean) as string[])
      : [];

  return {
    bouncedEmails,
    complaintEmails,
  };
}

emailWebhooks.post('/email/sns', async (c) => {
  const parsed = snsMessageSchema.safeParse(await c.req.json().catch(() => null));
  if (!parsed.success) {
    return c.json({ error: 'Invalid request data', details: z.treeifyError(parsed.error) }, 400);
  }
  const body = parsed.data;

  // API-041: only allowlisted topics, and only messages SNS itself signed.
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

  const { bouncedEmails, complaintEmails } = extractComplaintOrBounceEmails(body.Message);
  const impactedEmails = Array.from(new Set([...bouncedEmails, ...complaintEmails]));
  await markUsersUnverifiedByEmails(c.env.DB, impactedEmails);
  await markUsersEmailBouncedByEmails(c.env.DB, bouncedEmails);

  return c.json({ ok: true, updated: impactedEmails.length });
});

export default emailWebhooks;
