const VERIFY_URL = 'https://challenges.cloudflare.com/turnstile/v0/siteverify';

/**
 * Checks a Cloudflare Turnstile token from the newsletter form. Fails closed: a missing secret,
 * a missing token or a failed request all count as not verified.
 */
export async function verifyTurnstile(
  secret: string | undefined,
  token: unknown,
  ip: string | undefined,
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
    return res.ok && ((await res.json()) as { success?: boolean }).success === true;
  } catch (err) {
    console.error('[newsletter] Turnstile verification failed', err);
    return false;
  }
}
