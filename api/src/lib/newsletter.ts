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
 * Best-effort: failures are logged, never thrown, so they can't fail the signup.
 */
export async function subscribeToNewsletter(env: Env, email: string): Promise<void> {
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
      }),
    });
    if (!res.ok) {
      console.error(`[newsletter] Buttondown returned ${res.status}: ${await res.text()}`);
    }
  } catch (err) {
    console.error('[newsletter] Buttondown request failed', err);
  }
}
