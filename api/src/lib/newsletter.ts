import { Env } from '../types/bindings';

/**
 * Subscribes a new account's email to the newsletter (API-010). The address was just verified by
 * the signup flow and the box was ticked, so the row is created already confirmed. That also
 * resubscribes an address that unsubscribed, because ticking the box is a new opt-in. An address
 * suppressed by a hard bounce or complaint is never touched.
 *
 * The first location seen for an address wins, except that an account signup replaces a row the
 * landing form created: anyone can submit any address there, so the owner's own signup is the
 * better record.
 *
 * Best-effort: failures are logged, never thrown, so they can't fail the signup.
 */
export async function subscribeToNewsletter(
  env: Env,
  email: string,
  cf: Request['cf'],
): Promise<void> {
  if (!env.DONATE_DB) {
    console.log('[newsletter] DONATE_DB not set; skipping signup');
    return;
  }

  const country = (cf?.country as string | undefined) ?? null;
  const regionCode = (cf?.regionCode as string | undefined) ?? null;
  const region = (cf?.region as string | undefined) ?? null;

  try {
    // Every right-hand side in DO UPDATE reads the row as it was before the update.
    await env.DONATE_DB.prepare(
      `INSERT INTO newsletter_subscribers (email, country, region_code, region, source, confirmed_at)
       VALUES (?, ?, ?, ?, 'signup', unixepoch())
       ON CONFLICT(email) DO UPDATE SET
         country = CASE WHEN source = 'landing' THEN excluded.country ELSE country END,
         region_code = CASE WHEN source = 'landing' THEN excluded.region_code ELSE region_code END,
         region = CASE WHEN source = 'landing' THEN excluded.region ELSE region END,
         source = 'signup',
         confirmed_at = CASE
           WHEN confirmed_at IS NULL OR unsubscribed_at IS NOT NULL THEN unixepoch()
           ELSE confirmed_at
         END,
         unsubscribed_at = NULL
       WHERE suppressed_at IS NULL`,
    )
      .bind(email, country, regionCode, region)
      .run();
  } catch (err) {
    console.error('[newsletter] failed to subscribe', err);
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

    // Fill the region code and name together, and only for the stored country, so a row never
    // mixes values from different places.
    const canFillRegion =
      !row.region_code && !row.region && (!row.country || row.country === country);
    const nextCountry = row.country ?? country;
    const nextCode = canFillRegion ? regionCode : row.region_code;
    const nextRegion = canFillRegion ? ((cf?.region as string | undefined) ?? null) : row.region;
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
