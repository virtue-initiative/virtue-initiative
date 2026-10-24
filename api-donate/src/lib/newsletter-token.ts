const encoder = new TextEncoder();

export type NewsletterTokenPurpose = 'confirm' | 'unsubscribe';

function toBase64Url(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
}

function fromBase64Url(value: string): Uint8Array {
  const padded = value
    .replace(/-/g, '+')
    .replace(/_/g, '/')
    .padEnd(Math.ceil(value.length / 4) * 4, '=');
  return Uint8Array.from(atob(padded), (c) => c.charCodeAt(0));
}

function hmacKey(secret: string, usage: 'sign' | 'verify') {
  return crypto.subtle.importKey(
    'raw',
    encoder.encode(secret),
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    [usage],
  );
}

/**
 * Makes a stateless link token for an address: `base64url(email).issuedAt.base64url(hmac)`. The
 * purpose and `issuedAt` are part of the signed message, so a confirm token can't be used to
 * unsubscribe, and a confirm link is tied to the one email it was sent in (`issuedAt` is that
 * email's send time). Unsubscribe tokens use `issuedAt` 0 and never expire, since an unsubscribe
 * link in an old email has to keep working.
 */
export async function signNewsletterToken(
  secret: string,
  purpose: NewsletterTokenPurpose,
  email: string,
  issuedAt = 0,
): Promise<string> {
  const signature = await crypto.subtle.sign(
    'HMAC',
    await hmacKey(secret, 'sign'),
    encoder.encode(`${purpose}:${issuedAt}:${email}`),
  );
  return `${toBase64Url(encoder.encode(email))}.${issuedAt}.${toBase64Url(new Uint8Array(signature))}`;
}

/** Returns the address and issue time a token was signed for, or null if it isn't ours. */
export async function verifyNewsletterToken(
  secret: string,
  purpose: NewsletterTokenPurpose,
  token: string,
): Promise<{ email: string; issuedAt: number } | null> {
  const [emailPart, issuedPart, signaturePart, ...rest] = token.split('.');
  if (!emailPart || !issuedPart || !signaturePart || rest.length > 0) return null;
  if (!/^\d{1,12}$/.test(issuedPart)) return null;

  try {
    const email = new TextDecoder().decode(fromBase64Url(emailPart));
    const valid = await crypto.subtle.verify(
      'HMAC',
      await hmacKey(secret, 'verify'),
      fromBase64Url(signaturePart),
      encoder.encode(`${purpose}:${issuedPart}:${email}`),
    );
    return valid ? { email, issuedAt: Number(issuedPart) } : null;
  } catch {
    return null;
  }
}
