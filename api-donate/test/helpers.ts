import { env } from 'cloudflare:test';
import Stripe from 'stripe';

export const BASE = 'http://localhost';

export async function clearDB() {
  await env.DB.prepare('DELETE FROM donations').run();
  await env.DB.prepare('DELETE FROM stripe_events').run();
}

const signingStripe = new Stripe('sk_test_dummy', {
  httpClient: Stripe.createFetchHttpClient(),
});
const cryptoProvider = Stripe.createSubtleCryptoProvider();

/**
 * Build a Stripe webhook payload plus a valid `stripe-signature` header signed
 * with the test webhook secret, so the worker's signature check passes.
 */
export async function signedWebhook(event: Record<string, unknown>) {
  const payload = JSON.stringify(event);
  const signature = await signingStripe.webhooks.generateTestHeaderStringAsync({
    payload,
    secret: 'whsec_test_secret',
    cryptoProvider,
  });
  return { payload, signature };
}

export function checkoutCompletedEvent(
  overrides: Partial<{
    eventId: string;
    sessionId: string;
    mode: 'payment' | 'subscription';
    amountTotal: number;
    email: string;
    name: string;
    phone: string;
    customer: string;
    subscription: string | null;
  }> = {},
) {
  const {
    eventId = 'evt_test_1',
    sessionId = 'cs_test_1',
    mode = 'payment',
    amountTotal = 2500,
    email = 'donor@example.com',
    name = 'Donor Example',
    phone = '+15555550123',
    customer = 'cus_test_1',
    subscription = null,
  } = overrides;

  return {
    id: eventId,
    type: 'checkout.session.completed',
    data: {
      object: {
        id: sessionId,
        object: 'checkout.session',
        mode,
        amount_total: amountTotal,
        currency: 'usd',
        customer,
        subscription,
        customer_details: { email, name, phone },
      },
    },
  };
}
