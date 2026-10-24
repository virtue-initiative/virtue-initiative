const VERIFY_URL = 'https://challenges.cloudflare.com/turnstile/v0/siteverify';

/** The action the landing form's widget sets (`data-action`), so tokens from other forms fail. */
export const NEWSLETTER_TURNSTILE_ACTION = 'newsletter';

/**
 * Checks a Cloudflare Turnstile token from the newsletter form. Fails closed: a missing secret,
 * a missing token or a failed request all count as not verified. With `hostnames`, the token must
 * also have been issued on one of those hosts for the newsletter action, so a token minted for
 * another site that shares the widget can't be replayed here.
 */
export async function verifyTurnstile(
  secret: string | undefined,
  token: unknown,
  ip: string | undefined,
  hostnames?: string,
): Promise<boolean> {
  if (!secret) {
    console.error('[newsletter] TURNSTILE_SECRET_KEY is not set; rejecting signup');
    return false;
  }
  if (typeof token !== 'string' || token === '') return false;

  try {
    const res = await fetch(VERIFY_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ secret, response: token, ...(ip ? { remoteip: ip } : {}) }),
    });
    if (!res.ok) return false;
    const result = (await res.json()) as { success?: boolean; action?: string; hostname?: string };
    if (result.success !== true) return false;

    const allowed = (hostnames ?? '')
      .split(',')
      .map((h) => h.trim().toLowerCase())
      .filter(Boolean);
    if (allowed.length === 0) return true;
    return (
      result.action === NEWSLETTER_TURNSTILE_ACTION &&
      allowed.includes((result.hostname ?? '').toLowerCase())
    );
  } catch (err) {
    console.error('[newsletter] Turnstile verification failed', err);
    return false;
  }
}
