import { Hono } from 'hono';
import { z } from 'zod';
import { markUsersUnverifiedByEmails } from '../lib/db';
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
    notificationType?: string;
    bounce?: { bouncedRecipients?: Array<{ emailAddress?: string }> };
    complaint?: { complainedRecipients?: Array<{ emailAddress?: string }> };
  };

  if (parsed.notificationType === 'Bounce') {
    return (parsed.bounce?.bouncedRecipients ?? [])
      .map((recipient) => recipient.emailAddress?.trim().toLowerCase())
      .filter(Boolean) as string[];
  }

  if (parsed.notificationType === 'Complaint') {
    return (parsed.complaint?.complainedRecipients ?? [])
      .map((recipient) => recipient.emailAddress?.trim().toLowerCase())
      .filter(Boolean) as string[];
  }

  return [];
}

emailWebhooks.post('/email/sns', async (c) => {
  const body = snsEnvelopeSchema.parse(await c.req.json());

  if (body.Type === 'SubscriptionConfirmation' && body.SubscribeURL) {
    await fetch(body.SubscribeURL);
    return c.json({ ok: true, subscribed: true });
  }

  if (body.Type !== 'Notification' || !body.Message) {
    return c.json({ ok: true });
  }

  const emails = extractComplaintOrBounceEmails(body.Message);
  await markUsersUnverifiedByEmails(c.env.DB, emails);

  return c.json({ ok: true, updated: emails.length });
});

export default emailWebhooks;
