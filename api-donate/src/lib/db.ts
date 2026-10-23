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

export interface Goal {
  id: string;
  title: string;
  description: string | null;
  targetCents: number;
  raisedCents: number;
}

/**
 * List active fundraising goals.
 *
 * `raised_cents` is maintained by hand in the `goals` table; it is not derived
 * from the donations table.
 */
export async function getActiveGoals(db: D1Database): Promise<Goal[]> {
  const { results } = await db
    .prepare(
      `SELECT id, title, description, target_cents, raised_cents
       FROM goals WHERE active = 1 ORDER BY sort_order, created_at`,
    )
    .all<{
      id: string;
      title: string;
      description: string | null;
      target_cents: number;
      raised_cents: number;
    }>();

  return results.map((row) => ({
    id: row.id,
    title: row.title,
    description: row.description,
    targetCents: row.target_cents,
    raisedCents: row.raised_cents,
  }));
}

export interface NewsletterSubscriberRecord {
  email: string;
  country?: string;
  regionCode?: string;
  region?: string;
  source: 'landing' | 'signup';
}

/** Keeps the first location seen for an address, so a repeat signup can't overwrite it. */
export async function recordSubscriber(
  db: D1Database,
  subscriber: NewsletterSubscriberRecord,
): Promise<void> {
  await db
    .prepare(
      `INSERT INTO newsletter_subscribers (email, country, region_code, region, source)
       VALUES (?, ?, ?, ?, ?)
       ON CONFLICT(email) DO NOTHING`,
    )
    .bind(
      subscriber.email,
      subscriber.country ?? null,
      subscriber.regionCode ?? null,
      subscriber.region ?? null,
      subscriber.source,
    )
    .run();
}
