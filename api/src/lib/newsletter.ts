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
    console.log(`[newsletter] BUTTONDOWN_API_KEY not set; skipping signup for ${email}`);
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
 * metadata for us. The first location seen for an address wins.
 *
 * Best-effort: failures are logged, never thrown, so they can't fail the signup.
 */
export async function recordNewsletterRegion(
  env: Env,
  email: string,
  cf: Request['cf'],
): Promise<void> {
  if (!env.DONATE_DB) return;

  try {
    await env.DONATE_DB.prepare(
      `INSERT INTO newsletter_subscribers (email, country, region_code, region, source)
       VALUES (?, ?, ?, ?, 'signup')
       ON CONFLICT(email) DO NOTHING`,
    )
      .bind(
        email,
        (cf?.country as string | undefined) ?? null,
        (cf?.regionCode as string | undefined) ?? null,
        (cf?.region as string | undefined) ?? null,
      )
      .run();
  } catch (err) {
    console.error('[newsletter] failed to record subscriber region', err);
  }
}

/**
 * Fills in the region of an existing newsletter subscriber when they log in (API-016). It only
 * updates a row that API-010 or a backfill created, so users who never subscribed are never
 * recorded, and it never overwrites a region that is already set.
 *
 * Best-effort: failures are logged, never thrown, so they can't fail the login.
 */
export async function fillNewsletterRegion(
  env: Env,
  email: string,
  cf: Request['cf'],
): Promise<void> {
  const regionCode = cf?.regionCode as string | undefined;
  if (!env.DONATE_DB || !regionCode) return;

  try {
    await env.DONATE_DB.prepare(
      `UPDATE newsletter_subscribers
       SET country = ?, region_code = ?, region = ?
       WHERE email = ? AND region_code IS NULL`,
    )
      .bind(
        (cf?.country as string | undefined) ?? null,
        regionCode,
        (cf?.region as string | undefined) ?? null,
        email,
      )
      .run();
  } catch (err) {
    console.error('[newsletter] failed to fill subscriber region', err);
  }
}
