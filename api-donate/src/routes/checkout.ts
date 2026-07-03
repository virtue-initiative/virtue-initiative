import { zValidator } from '@hono/zod-validator';
import { Hono } from 'hono';
import { z } from 'zod';
import { createStripeClient } from '../lib/stripe';
import { Env, Variables } from '../types/bindings';

const checkout = new Hono<{ Bindings: Env; Variables: Variables }>();

const checkoutSchema = z.object({
  // Donation amount in whole US dollars. Capped to keep obviously-bogus values out of Stripe.
  amount: z.number().positive().max(100000),
  recurring: z.boolean(),
});

checkout.post('/checkout', zValidator('json', checkoutSchema), async (c) => {
  const { amount, recurring } = c.req.valid('json');
  const stripe = createStripeClient(c.env.STRIPE_SECRET_KEY);

  const unitAmount = Math.round(amount * 100);

  const session = await stripe.checkout.sessions.create({
    mode: recurring ? 'subscription' : 'payment',
    line_items: [
      {
        quantity: 1,
        price_data: {
          currency: 'usd',
          unit_amount: unitAmount,
          product_data: {
            name: recurring
              ? 'a monthly donation to The Virtue Initiative'
              : 'Donation to The Virtue Initiative',
          },
          ...(recurring ? { recurring: { interval: 'month' } } : {}),
        },
      },
    ],
    // Frame the flow as a donation rather than a purchase/subscription. Supported
    // for subscription mode since API version 2024-11-20.acacia (see lib/stripe.ts).
    submit_type: 'donate',
    // Collect everything we want from the donor inside Stripe Checkout so the
    // landing form can stay minimal.
    name_collection: { individual: { enabled: true, optional: false } },
    billing_address_collection: 'auto',
    phone_number_collection: { enabled: true },
    success_url: `${c.env.LANDING_URL}/donate/success?session_id={CHECKOUT_SESSION_ID}`,
    cancel_url: `${c.env.LANDING_URL}/donate`,
  });

  if (!session.url) {
    return c.json({ error: 'Failed to create checkout session' }, 502);
  }

  return c.json({ url: session.url });
});

export default checkout;
