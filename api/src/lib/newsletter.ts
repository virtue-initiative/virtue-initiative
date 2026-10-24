import { Env } from '../types/bindings';

const BUTTONDOWN_SUBSCRIBERS_URL = 'https://api.buttondown.com/v1/subscribers';

/**
 * Adds a new account's email to the Buttondown newsletter list (API-010).
 *
 * The address was just verified by the signup flow, so the subscriber is created as
 * `regular` and skips Buttondown's own confirmation email. No collision header is sent:
 * Buttondown then rejects an address it already knows (including one that unsubscribed)
 * instead of resubscribing it.
 *
 * `ipAddress` is the signing-up client's IP. Buttondown filters aggressively without it.
 *
 * Best-effort: failures are logged, never thrown, so they can't fail the signup.
 */
export async function subscribeToNewsletter(
  env: Env,
  email: string,
  ipAddress?: string,
): Promise<void> {
  const apiKey = env.BUTTONDOWN_API_KEY?.trim();
  if (!apiKey) {
    console.log('[newsletter] BUTTONDOWN_API_KEY not set; skipping signup');
    return;
  }

  try {
    const res = await fetch(BUTTONDOWN_SUBSCRIBERS_URL, {
      method: 'POST',
      headers: {
        Authorization: `Token ${apiKey}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        email_address: email,
        type: 'regular',
        referrer_url: `${env.APP_URL}/signup`,
        ...(ipAddress ? { ip_address: ipAddress } : {}),
      }),
    });
    if (!res.ok) {
      console.error(`[newsletter] Buttondown returned ${res.status}: ${await res.text()}`);
    }
  } catch (err) {
    console.error('[newsletter] Buttondown request failed', err);
  }
}

/**
 * Stores where a newsletter subscriber signed up from (API-010), since Buttondown keeps no
 * metadata for us. The first location seen for an address wins, except that an account signup
 * replaces a row the landing form created: anyone can submit any address there, so the owner's
 * own signup is the better record.
 *
 * Best-effort: failures are logged, never thrown, so they can't fail the signup.
 */
export async function recordNewsletterRegion(
  env: Env,
  email: string,
  cf: Request['cf'],
): Promise<void> {
  if (!env.DONATE_DB) return;

  const country = (cf?.country as string | undefined) ?? null;
  const regionCode = (cf?.regionCode as string | undefined) ?? null;
  const region = (cf?.region as string | undefined) ?? null;

  try {
    await env.DONATE_DB.prepare(
      `INSERT INTO newsletter_subscribers (email, country, region_code, region, source)
       VALUES (?, ?, ?, ?, 'signup')
       ON CONFLICT(email) DO UPDATE SET
         country = excluded.country,
         region_code = excluded.region_code,
         region = excluded.region,
         source = 'signup'
       WHERE source = 'landing'`,
    )
      .bind(email, country, regionCode, region)
      .run();
  } catch (err) {
    console.error('[newsletter] failed to record subscriber region', err);
  }
}

/**
 * Fills in the missing location of an account signup's newsletter row when they log in
 * (API-016). It only touches rows with `source = 'signup'` that API-010 or a backfill created,
 * so users who never subscribed are never recorded and an address someone else typed into the
 * landing form is never tied to an account. It never overwrites a value that is already set.
 *
 * Best-effort: failures are logged, never thrown, so they can't fail the login.
 */
export async function fillNewsletterRegion(
  env: Env,
  email: string,
  cf: Request['cf'],
): Promise<void> {
  const country = (cf?.country as string | undefined) ?? null;
  const regionCode = (cf?.regionCode as string | undefined) ?? null;
  if (!env.DONATE_DB || (!country && !regionCode)) return;

  try {
    // Read first: D1 writes are the expensive part, and nearly every login finds nothing to fill.
    const row = await env.DONATE_DB.prepare(
      `SELECT country, region_code, region FROM newsletter_subscribers
       WHERE email = ? AND source = 'signup'`,
    )
      .bind(email)
      .first<{ country: string | null; region_code: string | null; region: string | null }>();
    if (!row) return;

    // Only pair a region with the stored country when they agree.
    const sameCountry = !row.country || row.country === country;
    const nextCountry = row.country ?? country;
    const nextCode = row.region_code ?? (sameCountry ? regionCode : null);
    const nextRegion =
      row.region ?? (sameCountry ? ((cf?.region as string | undefined) ?? null) : null);
    if (nextCountry === row.country && nextCode === row.region_code && nextRegion === row.region) {
      return;
    }

    await env.DONATE_DB.prepare(
      `UPDATE newsletter_subscribers SET country = ?, region_code = ?, region = ?
       WHERE email = ? AND source = 'signup'`,
    )
      .bind(nextCountry, nextCode, nextRegion, email)
      .run();
  } catch (err) {
    console.error('[newsletter] failed to fill subscriber region', err);
  }
}
