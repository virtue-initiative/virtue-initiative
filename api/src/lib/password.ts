export const HASH_PARAMS_VERSION = 'argon2id-v1';

export const CURRENT_HASH_PARAMS = {
  version: HASH_PARAMS_VERSION,
  algorithm: 'argon2id',
  memory_cost_kib: 131_072,
  time_cost: 5,
  parallelism: 1,
  salt_length: 16,
  hkdf_hash: 'sha256',
} as const;

function bytesToHex(bytes: Uint8Array) {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

function hexToBytes(hex: string) {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.slice(i, i + 2), 16);
  }
  return bytes;
}

function toBytes(value: ArrayBuffer | Uint8Array) {
  return value instanceof Uint8Array ? value : new Uint8Array(value);
}

export function generatePasswordSalt() {
  return crypto.getRandomValues(new Uint8Array(CURRENT_HASH_PARAMS.salt_length));
}

let cachedSaltKey: { secret: string; key: Promise<CryptoKey> } | undefined;

/** Imports the HMAC key once per isolate, and again if the secret changes. */
function saltKeyFor(secret: string) {
  if (cachedSaltKey?.secret !== secret) {
    const key = crypto.subtle.importKey(
      'raw',
      new TextEncoder().encode(secret),
      { name: 'HMAC', hash: 'SHA-256' },
      false,
      ['sign'],
    );
    cachedSaltKey = { secret, key };
    key.catch(() => {
      if (cachedSaltKey?.key === key) cachedSaltKey = undefined;
    });
  }
  return cachedSaltKey.key;
}

/** API-015: the salt shown for an email with no account, stable as a real account's salt is. */
export async function madeUpPasswordSalt(secret: string | undefined, normalizedEmail: string) {
  if (!secret) {
    console.error('[login-material] LOGIN_SALT_SECRET is not set; using a random made-up salt');
    return generatePasswordSalt();
  }
  const key = await saltKeyFor(secret);
  const mac = await crypto.subtle.sign('HMAC', key, new TextEncoder().encode(normalizedEmail));
  return new Uint8Array(mac).slice(0, CURRENT_HASH_PARAMS.salt_length);
}

export async function hashPasswordAuth(passwordAuth: ArrayBuffer | Uint8Array) {
  const hashBuffer = await crypto.subtle.digest('SHA-256', toBytes(passwordAuth));
  return bytesToHex(new Uint8Array(hashBuffer));
}

export async function verifyPasswordAuth(
  passwordAuth: ArrayBuffer | Uint8Array,
  storedHash: string,
): Promise<boolean> {
  try {
    const expected = hexToBytes(storedHash);
    const hashBuffer = await crypto.subtle.digest('SHA-256', toBytes(passwordAuth));
    const actual = new Uint8Array(hashBuffer);
    if (actual.length !== expected.length) return false;
    let diff = 0;
    for (let i = 0; i < actual.length; i++) diff |= actual[i] ^ expected[i];
    return diff === 0;
  } catch {
    return false;
  }
}
