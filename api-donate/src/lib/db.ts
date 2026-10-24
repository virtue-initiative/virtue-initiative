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
}

// A repeat signup form post won't send another confirmation email within this window.
const CONFIRMATION_RESEND_SECONDS = 60 * 60;
const DAY_SECONDS = 24 * 60 * 60;

export type SubscriptionRequest =
  // A confirmation email should go out, with this send time baked into its link.
  | { status: 'send'; sentAt: number }
  // Already subscribed, suppressed or sent too recently. The visitor can't tell which.
  | { status: 'skipped' }
  // Too many confirmation emails went out in the last day.
  | { status: 'capped' };

const nowSeconds = () => Math.floor(Date.now() / 1000);

/** How many addresses were sent a confirmation email in the last day. */
export async function countRecentConfirmations(db: D1Database): Promise<number> {
  const row = await db
    .prepare(`SELECT COUNT(*) AS n FROM newsletter_subscribers WHERE confirmation_sent_at > ?`)
    .bind(nowSeconds() - DAY_SECONDS)
    .first<{ n: number }>();
  return row?.n ?? 0;
}

/**
 * Records a landing-page signup, keeping the first location seen for an address, and claims the
 * right to email it a confirmation link. Never for an address that is suppressed (it hard-bounced
 * or complained) or already subscribed, and not again within an hour. `dailyCap` caps the
 * confirmation emails sent across all addresses, so the form can't be used to send mass mail.
 */
export async function requestSubscription(
  db: D1Database,
  subscriber: NewsletterSubscriberRecord,
  dailyCap: number,
): Promise<SubscriptionRequest> {
  await db
    .prepare(
      `INSERT INTO newsletter_subscribers (email, country, region_code, region, source)
       VALUES (?, ?, ?, ?, 'landing')
       ON CONFLICT(email) DO NOTHING`,
    )
    .bind(
      subscriber.email,
      subscriber.country ?? null,
      subscriber.regionCode ?? null,
      subscriber.region ?? null,
    )
    .run();

  if ((await countRecentConfirmations(db)) >= dailyCap) return { status: 'capped' };

  // Atomic, so two concurrent posts can't both send.
  const sentAt = nowSeconds();
  const result = await db
    .prepare(
      `UPDATE newsletter_subscribers SET confirmation_sent_at = ?1
       WHERE email = ?2
         AND suppressed_at IS NULL
         AND (confirmed_at IS NULL OR unsubscribed_at IS NOT NULL)
         AND (confirmation_sent_at IS NULL OR confirmation_sent_at <= ?1 - ?3)`,
    )
    .bind(sentAt, subscriber.email, CONFIRMATION_RESEND_SECONDS)
    .run();

  return (result.meta.changes ?? 0) > 0 ? { status: 'send', sentAt } : { status: 'skipped' };
}

/** Gives back the claim from `requestSubscription` when the email could not be sent. */
export async function releaseConfirmation(
  db: D1Database,
  email: string,
  sentAt: number,
): Promise<void> {
  await db
    .prepare(
      `UPDATE newsletter_subscribers SET confirmation_sent_at = NULL
       WHERE email = ? AND confirmation_sent_at = ?`,
    )
    .bind(email, sentAt)
    .run();
}

/**
 * Whether a confirm link is still good: it must belong to the latest confirmation email, the
 * address must not be suppressed, and the subscriber must not have unsubscribed since it was
 * sent. That last rule stops an old email, a forward or a mail scanner from resubscribing
 * someone who opted out.
 */
export async function isConfirmationValid(
  db: D1Database,
  email: string,
  issuedAt: number,
): Promise<boolean> {
  const row = await db
    .prepare(
      `SELECT 1 AS ok FROM newsletter_subscribers
       WHERE email = ?1 AND suppressed_at IS NULL AND confirmation_sent_at = ?2
         AND (unsubscribed_at IS NULL OR unsubscribed_at < ?2)`,
    )
    .bind(email, issuedAt)
    .first();
  return row !== null;
}

/** Marks an address confirmed, if the link is still valid. Re-confirming clears an unsubscribe. */
export async function confirmSubscriber(
  db: D1Database,
  email: string,
  issuedAt: number,
): Promise<boolean> {
  const result = await db
    .prepare(
      `UPDATE newsletter_subscribers SET confirmed_at = unixepoch(), unsubscribed_at = NULL
       WHERE email = ?1 AND suppressed_at IS NULL AND confirmation_sent_at = ?2
         AND (unsubscribed_at IS NULL OR unsubscribed_at < ?2)`,
    )
    .bind(email, issuedAt)
    .run();
  return (result.meta.changes ?? 0) > 0;
}

export async function unsubscribeSubscriber(db: D1Database, email: string): Promise<void> {
  await db
    .prepare(
      `UPDATE newsletter_subscribers SET unsubscribed_at = unixepoch()
       WHERE email = ? AND unsubscribed_at IS NULL`,
    )
    .bind(email)
    .run();
}

/**
 * Permanently stops mail to addresses that hard-bounced or complained. Unlike an unsubscribe,
 * nothing clears this: not a confirm link, not a new signup, not an account opt-in.
 */
export async function suppressSubscribers(db: D1Database, emails: string[]): Promise<void> {
  for (const email of emails) {
    await db
      .prepare(
        `UPDATE newsletter_subscribers SET suppressed_at = COALESCE(suppressed_at, unixepoch())
         WHERE email = ?`,
      )
      .bind(email)
      .run();
  }
}
