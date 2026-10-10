import { Hono } from 'hono';
import { v4 as uuidv4 } from 'uuid';
import { z } from 'zod';
import {
  createEmailBounce,
  markUsersEmailBouncedByEmails,
  markUsersUnverifiedByEmails,
  setBounceOnOutstandingEmailTokens,
  setEmailTokenBounce,
  type EmailBounceReason,
} from '../lib/db';
import { EMAIL_TOKEN_TAG } from '../lib/email';
import { Env, Variables } from '../types/bindings';

const emailWebhooks = new Hono<{ Bindings: Env; Variables: Variables }>();

const snsEnvelopeSchema = z.object({
  Type: z.string(),
  TopicArn: z.string().optional(),
  SubscribeURL: z.string().optional(),
  Message: z.string().optional(),
});

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function extractComplaintOrBounceEmails(message: string) {
  const parsed = JSON.parse(message) as {
    eventType?: string;
    mail?: { tags?: Record<string, string[] | undefined> };
    bounce?: { bounceType?: string; bouncedRecipients?: Array<{ emailAddress?: string }> };
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

  // API-056: only `Permanent` blocks the address; `Transient` and
  // `Undetermined` may deliver next time.
  const bounceReason: EmailBounceReason =
    parsed.bounce?.bounceType === 'Permanent' ? 'permanent' : 'temporary';

  const taggedTokenId = parsed.mail?.tags?.[EMAIL_TOKEN_TAG]?.[0];

  return {
    bouncedEmails: Array.from(new Set(bouncedEmails)),
    complaintEmails,
    bounceReason,
    emailTokenId: taggedTokenId && UUID_PATTERN.test(taggedTokenId) ? taggedTokenId : null,
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

  const { bouncedEmails, complaintEmails, bounceReason, emailTokenId } =
    extractComplaintOrBounceEmails(body.Message);
  const impactedEmails = Array.from(new Set([...bouncedEmails, ...complaintEmails]));

  const now = Date.now();
  for (const email of bouncedEmails) {
    const bounceId = uuidv4();
    await createEmailBounce(c.env.DB, {
      id: bounceId,
      email,
      bounced_at: now,
      reason: bounceReason,
    });
    if (emailTokenId) {
      await setEmailTokenBounce(c.env.DB, { token_id: emailTokenId, email, bounce_id: bounceId });
    }
    if (bounceReason === 'permanent') {
      // The address is dead, so nothing else waiting on it was delivered either.
      await setBounceOnOutstandingEmailTokens(c.env.DB, email, bounceId);
    }
  }

  // A temporary bounce says nothing about whether the address is real, so it
  // doesn't cost the user their verified status.
  const unverifiedEmails =
    bounceReason === 'permanent' ? impactedEmails : Array.from(new Set(complaintEmails));
  await markUsersUnverifiedByEmails(c.env.DB, unverifiedEmails);
  await markUsersEmailBouncedByEmails(c.env.DB, bounceReason === 'permanent' ? bouncedEmails : []);

  return c.json({ ok: true, updated: impactedEmails.length });
});

export default emailWebhooks;
