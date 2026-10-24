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
 * Makes a stateless link token for an address: `base64url(email).base64url(hmac)`. The purpose is
 * part of the signed message, so a confirm token can't be used to unsubscribe or the reverse.
 * Tokens don't expire, since an unsubscribe link in an old email has to keep working.
 */
export async function signNewsletterToken(
  secret: string,
  purpose: NewsletterTokenPurpose,
  email: string,
): Promise<string> {
  const signature = await crypto.subtle.sign(
    'HMAC',
    await hmacKey(secret, 'sign'),
    encoder.encode(`${purpose}:${email}`),
  );
  return `${toBase64Url(encoder.encode(email))}.${toBase64Url(new Uint8Array(signature))}`;
}

/** Returns the address a token was issued for, or null if it is malformed or not signed by us. */
export async function verifyNewsletterToken(
  secret: string,
  purpose: NewsletterTokenPurpose,
  token: string,
): Promise<string | null> {
  const [emailPart, signaturePart, ...rest] = token.split('.');
  if (!emailPart || !signaturePart || rest.length > 0) return null;

  try {
    const email = new TextDecoder().decode(fromBase64Url(emailPart));
    const valid = await crypto.subtle.verify(
      'HMAC',
      await hmacKey(secret, 'verify'),
      fromBase64Url(signaturePart),
      encoder.encode(`${purpose}:${email}`),
    );
    return valid ? email : null;
  } catch {
    return null;
  }
}
