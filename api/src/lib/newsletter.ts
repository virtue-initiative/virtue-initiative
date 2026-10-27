import { Env } from '../types/bindings';

// SES configuration set the newsletter is sent through (api-donate/scripts/newsletter.ts). SES
// tags each bounce and complaint event with it (API-041).
export const NEWSLETTER_SES_CONFIG_SET = 'newsletter';

// At most one confirmation email per address in this window (API-058).
const CONFIRMATION_RESEND_SECONDS = 60 * 60;

export interface RequestLocation {
  country: string | null;
  region: string | null;
}

/** Cloudflare's geolocation of the client: ISO country code and subdivision (US state) code. */
export function requestLocation(request: Request): RequestLocation {
  const cf = request.cf as IncomingRequestCfProperties | undefined;
  return { country: cf?.country ?? null, region: cf?.regionCode ?? null };
}

/**
 * Adds a just-verified account email to the newsletter list as confirmed (API-010). An existing
 * unconfirmed row becomes confirmed and gets a missing location filled in. One that unsubscribed
 * is left alone.
 *
 * Best-effort: failures are logged, never thrown, so they can't fail a signup.
 */
export async function addConfirmedNewsletterSignup(
  env: Env,
  email: string,
  location: RequestLocation,
): Promise<void> {
  try {
    await env.DONATE_DB.prepare(
      `INSERT INTO newsletter_signups (email, country, region, confirmed_at)
       VALUES (?, ?, ?, unixepoch())
       ON CONFLICT (email) DO UPDATE SET
         confirmed_at = coalesce(confirmed_at, excluded.confirmed_at),
         country = coalesce(country, excluded.country),
         region = CASE WHEN country IS NULL THEN excluded.region ELSE region END
       WHERE unsubscribed_at IS NULL`,
    )
      .bind(email, location.country, location.region)
      .run();
  } catch (err) {
    console.error('[newsletter] failed to record signup', err);
  }
}

/**
 * Adds an email from the landing form, unconfirmed (API-058). Returns the row's token when a
 * confirmation email should go out now, or null when the row is already confirmed, unsubscribed,
 * or was sent one within the last hour. Claiming the send here keeps two quick posts from both
 * sending.
 */
export async function claimNewsletterConfirmation(
  env: Env,
  email: string,
  location: RequestLocation,
): Promise<string | null> {
  await env.DONATE_DB.prepare(
    'INSERT OR IGNORE INTO newsletter_signups (email, country, region) VALUES (?, ?, ?)',
  )
    .bind(email, location.country, location.region)
    .run();
  const row = await env.DONATE_DB.prepare(
    `UPDATE newsletter_signups SET confirmation_sent_at = unixepoch()
     WHERE email = ? AND confirmed_at IS NULL AND unsubscribed_at IS NULL
       AND (confirmation_sent_at IS NULL OR confirmation_sent_at <= unixepoch() - ?)
     RETURNING token`,
  )
    .bind(email, CONFIRMATION_RESEND_SECONDS)
    .first<{ token: string }>();
  return row?.token ?? null;
}

/** API-059. Returns false when the token matches no row that is still subscribed. */
export async function confirmNewsletterSignup(env: Env, token: string): Promise<boolean> {
  const row = await env.DONATE_DB.prepare(
    `UPDATE newsletter_signups SET confirmed_at = coalesce(confirmed_at, unixepoch())
     WHERE token = ? AND unsubscribed_at IS NULL RETURNING email`,
  )
    .bind(token)
    .first();
  return row !== null;
}

/** API-060. Returns false when the token matches no row. */
export async function unsubscribeNewsletterToken(env: Env, token: string): Promise<boolean> {
  const row = await env.DONATE_DB.prepare(
    `UPDATE newsletter_signups SET unsubscribed_at = coalesce(unsubscribed_at, unixepoch())
     WHERE token = ? RETURNING email`,
  )
    .bind(token)
    .first();
  return row !== null;
}

/** Unsubscribes addresses that hard-bounced or complained (API-041). */
export async function unsubscribeNewsletterEmails(env: Env, emails: string[]): Promise<void> {
  if (emails.length === 0) return;
  await env.DONATE_DB.batch(
    emails.map((email) =>
      env.DONATE_DB.prepare(
        'UPDATE newsletter_signups SET unsubscribed_at = coalesce(unsubscribed_at, unixepoch()) WHERE email = ?',
      ).bind(email),
    ),
  );
}

/**
 * Fills in the location of a subscriber that has none yet, such as one imported from
 * Buttondown, when they log in (API-016, API-035). Best-effort, like addConfirmedNewsletterSignup.
 */
export async function fillNewsletterLocation(
  env: Env,
  email: string,
  location: RequestLocation,
): Promise<void> {
  if (!location.country) return;
  try {
    await env.DONATE_DB.prepare(
      'UPDATE newsletter_signups SET country = ?, region = ? WHERE email = ? AND country IS NULL',
    )
      .bind(location.country, location.region, email)
      .run();
  } catch (err) {
    console.error('[newsletter] failed to fill location', err);
  }
}
