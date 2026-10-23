import { Env } from '../types/bindings';

const BUTTONDOWN_SUBSCRIBERS_URL = 'https://api.buttondown.com/v1/subscribers';

/**
 * Adds an address to the Buttondown list as `unactivated`, so Buttondown sends its own
 * confirmation email before subscribing it. `ip` is the visitor's IP, which Buttondown uses to
 * validate the signup. Failures are logged, never thrown.
 */
export async function subscribeToButtondown(env: Env, email: string, ip?: string): Promise<void> {
  const apiKey = env.BUTTONDOWN_API_KEY?.trim();
  if (!apiKey) {
    console.log(`[newsletter] BUTTONDOWN_API_KEY not set; skipping signup for ${email}`);
    return;
  }

  try {
    const res = await fetch(BUTTONDOWN_SUBSCRIBERS_URL, {
      method: 'POST',
      headers: { Authorization: `Token ${apiKey}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email_address: email,
        type: 'unactivated',
        referrer_url: env.LANDING_URL,
        ...(ip ? { ip_address: ip } : {}),
      }),
    });
    if (!res.ok) {
      console.error(`[newsletter] Buttondown returned ${res.status}: ${await res.text()}`);
    }
  } catch (err) {
    console.error('[newsletter] Buttondown request failed', err);
  }
}
