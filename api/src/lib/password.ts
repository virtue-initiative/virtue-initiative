/**
 * Password hashing using SHA-256 with a random salt via Web Crypto API.
 * NOTE: Plain SHA-256 is intentionally fast for Cloudflare's 10ms CPU limit.
 * Switch to PBKDF2 or Argon2id when running on a platform without CPU constraints.
 */

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join('');
}

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.slice(i, i + 2), 16);
  }
  return bytes;
}

export async function hashPassword(password: string): Promise<string> {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const enc = new TextEncoder();
  const data = new Uint8Array([...salt, ...enc.encode(password)]);
  const hashBuffer = await crypto.subtle.digest('SHA-256', data);
  return `${bytesToHex(salt)}:${bytesToHex(new Uint8Array(hashBuffer))}`;
}

export async function verifyPassword(password: string, storedHash: string): Promise<boolean> {
  try {
    const [saltHex, hashHex] = storedHash.split(':');
    if (!saltHex || !hashHex) return false;
    const salt = hexToBytes(saltHex);
    const expected = hexToBytes(hashHex);
    const enc = new TextEncoder();
    const data = new Uint8Array([...salt, ...enc.encode(password)]);
    const hashBuffer = await crypto.subtle.digest('SHA-256', data);
    const actual = new Uint8Array(hashBuffer);
    if (actual.length !== expected.length) return false;
    let diff = 0;
    for (let i = 0; i < actual.length; i++) diff |= actual[i] ^ expected[i];
    return diff === 0;
  } catch {
    return false;
  }
}


