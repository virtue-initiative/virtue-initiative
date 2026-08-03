export interface DonationRecord {
  id: string;
  stripeCustomerId: string | null;
  stripeSubscriptionId: string | null;
  email: string | null;
  name: string | null;
  phone: string | null;
  amountCents: number;
  currency: string;
  recurring: boolean;
  status: string;
}

/**
 * Record (or update) a donation from a completed Checkout session.
 *
 * Upserts on the Checkout session id so a webhook replay of the same event is a
 * no-op rather than a duplicate row.
 */
export async function recordDonation(db: D1Database, donation: DonationRecord): Promise<void> {
  await db
    .prepare(
      `INSERT INTO donations (
         id, stripe_customer_id, stripe_subscription_id, email, name, phone,
         amount_cents, currency, recurring, status
       ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT(id) DO UPDATE SET
         stripe_customer_id = excluded.stripe_customer_id,
         stripe_subscription_id = excluded.stripe_subscription_id,
         email = excluded.email,
         name = excluded.name,
         phone = excluded.phone,
         amount_cents = excluded.amount_cents,
         currency = excluded.currency,
         recurring = excluded.recurring,
         status = excluded.status`,
    )
    .bind(
      donation.id,
      donation.stripeCustomerId,
      donation.stripeSubscriptionId,
      donation.email,
      donation.name,
      donation.phone,
      donation.amountCents,
      donation.currency,
      donation.recurring ? 1 : 0,
      donation.status,
    )
    .run();
}

/** Update the status of every donation tied to a subscription (e.g. on cancellation). */
export async function updateDonationStatusBySubscription(
  db: D1Database,
  subscriptionId: string,
  status: string,
): Promise<void> {
  await db
    .prepare(`UPDATE donations SET status = ? WHERE stripe_subscription_id = ?`)
    .bind(status, subscriptionId)
    .run();
}

/**
 * Record that a Stripe event id has been processed.
 *
 * Returns `true` if this is the first time we've seen the event, or `false` if
 * it was already recorded — callers use this to make webhook handling idempotent.
 */
export async function markStripeEventProcessed(
  db: D1Database,
  eventId: string,
  eventType: string,
): Promise<boolean> {
  const result = await db
    .prepare(`INSERT OR IGNORE INTO stripe_events (id, type) VALUES (?, ?)`)
    .bind(eventId, eventType)
    .run();

  return (result.meta.changes ?? 0) > 0;
}
