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
import { NEWSLETTER_SES_CONFIG_SET, unsubscribeNewsletterEmails } from '../lib/newsletter';
import { isSnsUrl, parseTopicArnAllowlist, snsMessageSchema, verifySnsSignature } from '../lib/sns';
import { Env, Variables } from '../types/bindings';

const emailWebhooks = new Hono<{ Bindings: Env; Variables: Variables }>();

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
    // SES tags every event with the configuration set the message went through.
    isNewsletter: parsed.mail?.tags?.['ses:configuration-set']?.[0] === NEWSLETTER_SES_CONFIG_SET,
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

  const { bouncedEmails, complaintEmails, bounceReason, emailTokenId, isNewsletter } =
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

  const permanentlyBounced = bounceReason === 'permanent' ? bouncedEmails : [];
  // A temporary bounce says nothing about whether the address is real, so it
  // doesn't cost the user their verified status. Nor does marking a newsletter as spam.
  const unverifiedEmails = Array.from(
    new Set([...permanentlyBounced, ...(isNewsletter ? [] : complaintEmails)]),
  );
  await markUsersUnverifiedByEmails(c.env.DB, unverifiedEmails);
  await markUsersEmailBouncedByEmails(c.env.DB, permanentlyBounced);
  await unsubscribeNewsletterEmails(c.env, [...permanentlyBounced, ...complaintEmails]);

  return c.json({ ok: true, updated: impactedEmails.length });
});

export default emailWebhooks;
