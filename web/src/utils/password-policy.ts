export const MIN_PASSWORD_LENGTH = 12;

const HIBP_RANGE_URL = 'https://api.pwnedpasswords.com/range/';

/** Returns an error message when the password is too short, or null when it passes. */
export function passwordLengthError(password: string): string | null {
  return password.length >= MIN_PASSWORD_LENGTH
    ? null
    : `Use at least ${MIN_PASSWORD_LENGTH} characters.`;
}

async function sha1Hex(value: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-1', new TextEncoder().encode(value));
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
    .toUpperCase();
}

/**
 * Looks the password up in the Have I Been Pwned breach corpus using its
 * k-anonymity range search. Only the first five hex characters of the SHA-1
 * hash leave the browser, never the password or the full hash.
 *
 * Returns the number of breaches the password appears in, 0 when it is absent,
 * or null when the lookup could not be completed. This check is advisory, so it
 * fails open rather than blocking the user.
 */
export async function checkPwnedPassword(
  password: string,
  signal?: AbortSignal,
): Promise<number | null> {
  try {
    const hash = await sha1Hex(password);
    const prefix = hash.slice(0, 5);
    const suffix = hash.slice(5);

    const response = await fetch(`${HIBP_RANGE_URL}${prefix}`, {
      headers: { 'Add-Padding': 'true' },
      signal,
    });
    if (!response.ok) return null;

    const body = await response.text();
    for (const line of body.split('\n')) {
      const [lineSuffix, count] = line.trim().split(':');
      if (lineSuffix !== suffix) continue;
      const parsed = Number.parseInt(count ?? '', 10);
      return Number.isFinite(parsed) ? parsed : null;
    }
    return 0;
  } catch {
    return null;
  }
}
