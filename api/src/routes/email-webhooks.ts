import { Hono } from 'hono';
import { z } from 'zod';
import { markUsersEmailBouncedByEmails, markUsersUnverifiedByEmails } from '../lib/db';
import { Env, Variables } from '../types/bindings';

const emailWebhooks = new Hono<{ Bindings: Env; Variables: Variables }>();

const snsEnvelopeSchema = z.object({
  Type: z.string(),
  TopicArn: z.string().optional(),
  SubscribeURL: z.string().optional(),
  Message: z.string().optional(),
});

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
  const data = await c.req.json();
  const body = snsEnvelopeSchema.parse(data);

  if (body.Type === 'SubscriptionConfirmation' && body.SubscribeURL) {
    await fetch(body.SubscribeURL);
    return c.json({ ok: true, subscribed: true });
  }

  if (body.Type !== 'Notification' || !body.Message) {
    return c.json({ ok: true });
  }

  const { bouncedEmails, complaintEmails } = extractComplaintOrBounceEmails(body.Message);
  const impactedEmails = Array.from(new Set([...bouncedEmails, ...complaintEmails]));
  await markUsersUnverifiedByEmails(c.env.DB, impactedEmails);
  await markUsersEmailBouncedByEmails(c.env.DB, bouncedEmails);

  return c.json({ ok: true, updated: impactedEmails.length });
});

export default emailWebhooks;
