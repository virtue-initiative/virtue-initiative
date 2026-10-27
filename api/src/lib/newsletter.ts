import { Env } from '../types/bindings';

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
 * Adds an email to the newsletter list in the donations database (API-010, API-058).
 *
 * An address already on the list keeps its row, so one that unsubscribed stays unsubscribed.
 * Only a missing location is filled in.
 *
 * Best-effort: failures are logged, never thrown, so they can't fail a signup.
 */
export async function addNewsletterSignup(
  env: Env,
  email: string,
  location: RequestLocation,
): Promise<void> {
  try {
    await env.DONATE_DB.prepare(
      `INSERT INTO newsletter_signups (email, country, region) VALUES (?, ?, ?)
       ON CONFLICT (email) DO UPDATE SET country = excluded.country, region = excluded.region
       WHERE newsletter_signups.country IS NULL`,
    )
      .bind(email, location.country, location.region)
      .run();
  } catch (err) {
    console.error('[newsletter] failed to record signup', err);
  }
}

/**
 * Fills in the location of a subscriber that has none yet, such as one imported from
 * Buttondown, when they log in (API-016, API-035). Best-effort, like addNewsletterSignup.
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
