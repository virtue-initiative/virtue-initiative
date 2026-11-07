const SITEVERIFY_URL = 'https://challenges.cloudflare.com/turnstile/v0/siteverify';

/**
 * Checks a Cloudflare Turnstile token (API-009, API-058). Fails closed: a missing secret, a
 * missing token or a failed request all count as not verified.
 */
export async function verifyTurnstile(
  secret: string | undefined,
  token: unknown,
  ip: string | undefined,
): Promise<boolean> {
  if (!secret) {
    console.error('[turnstile] TURNSTILE_SECRET_KEY is not set; rejecting request');
    return false;
  }
  if (typeof token !== 'string' || token === '') return false;

  try {
    const res = await fetch(SITEVERIFY_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ secret, response: token, ...(ip ? { remoteip: ip } : {}) }),
    });
    if (!res.ok) return false;
    const result = (await res.json()) as { success?: boolean };
    return result.success === true;
  } catch (err) {
    console.error('[turnstile] verification failed', err);
    return false;
  }
}
