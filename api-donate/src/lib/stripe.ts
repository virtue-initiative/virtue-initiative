import Stripe from 'stripe';

/**
 * Build a Stripe client configured for the Cloudflare Workers runtime.
 *
 * Workers has no Node `http` module, so Stripe must use a fetch-based HTTP
 * client. Webhook signature verification likewise cannot use Node crypto, so
 * callers pair this with the SubtleCrypto provider below and
 * `constructEventAsync` (see `routes/webhook.ts`).
 */
export function createStripeClient(secretKey: string): Stripe {
  return new Stripe(secretKey, {
    httpClient: Stripe.createFetchHttpClient(),
  });
}

// SubtleCrypto-backed provider for async webhook signature verification on Workers.
export const webhookCryptoProvider = Stripe.createSubtleCryptoProvider();
