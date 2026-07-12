import { Hono } from 'hono';
import type Stripe from 'stripe';
import {
  markStripeEventProcessed,
  recordDonation,
  updateDonationStatusBySubscription,
} from '../lib/db';
import { createStripeClient, webhookCryptoProvider } from '../lib/stripe';
import { Env, Variables } from '../types/bindings';

const webhook = new Hono<{ Bindings: Env; Variables: Variables }>();

function asId(value: string | { id: string } | null | undefined): string | null {
  if (!value) return null;
  return typeof value === 'string' ? value : value.id;
}

webhook.post('/webhook', async (c) => {
  const signature = c.req.header('stripe-signature');
  if (!signature) {
    return c.json({ error: 'Missing stripe-signature header' }, 400);
  }

  const stripe = createStripeClient(c.env.STRIPE_SECRET_KEY);
  const payload = await c.req.text();

  let event: Stripe.Event;
  try {
    event = await stripe.webhooks.constructEventAsync(
      payload,
      signature,
      c.env.STRIPE_WEBHOOK_SECRET,
      undefined,
      webhookCryptoProvider,
    );
  } catch (err) {
    console.error('Stripe webhook signature verification failed', err);
    return c.json({ error: 'Invalid signature' }, 400);
  }

  // Idempotency: ignore events we've already handled (Stripe retries deliveries).
  const isNew = await markStripeEventProcessed(c.env.DB, event.id, event.type);
  if (!isNew) {
    return c.json({ ok: true, duplicate: true });
  }

  switch (event.type) {
    case 'checkout.session.completed': {
      const session = event.data.object;
      await recordDonation(c.env.DB, {
        id: session.id,
        stripeCustomerId: asId(session.customer),
        stripeSubscriptionId: asId(session.subscription),
        email: session.customer_details?.email ?? null,
        name: session.customer_details?.name ?? null,
        phone: session.customer_details?.phone ?? null,
        amountCents: session.amount_total ?? 0,
        currency: session.currency ?? 'usd',
        recurring: session.mode === 'subscription',
        status: 'completed',
      });
      break;
    }
    case 'customer.subscription.deleted': {
      const subscription = event.data.object;
      await updateDonationStatusBySubscription(c.env.DB, subscription.id, 'cancelled');
      break;
    }
    default:
      // Other event types are acknowledged but not acted on.
      break;
  }

  return c.json({ ok: true });
});

export default webhook;
