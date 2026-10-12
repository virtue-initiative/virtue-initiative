import { decode, encode } from '@msgpack/msgpack';
import { decodeBase64, encodeBase64 } from './encoding';
import { bytesToUuid, uuidToBytes } from './db';

// batches.access_keys holds one HPKE-wrapped batch key per recipient. It is
// stored as msgpack([[user id (16 bytes), wrapped key], ...]), which is about a
// third smaller than the `{ "<uuid>": "<base64>" }` JSON the device uploads.

function toBytes(value: unknown): Uint8Array | null {
  if (value instanceof Uint8Array) return value;
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (Array.isArray(value)) return Uint8Array.from(value as number[]);
  return null;
}

export function encodeAccessKeys(accessKeys: Record<string, string>): Uint8Array {
  return encode(
    Object.entries(accessKeys).map(([userId, wrappedKey]) => [
      new Uint8Array(uuidToBytes(userId)),
      new Uint8Array(decodeBase64(wrappedKey)),
    ]),
  );
}

/** Returns the base64 wrapped key stored for `userId`, or null if there is none. */
export function wrappedKeyForUser(stored: unknown, userId: string): string | null {
  try {
    // Rows written before the column became msgpack hold the uploaded JSON as
    // text. They age out with the 30-day batch retention.
    if (typeof stored === 'string') {
      return (JSON.parse(stored) as Record<string, string>)[userId] ?? null;
    }

    const bytes = toBytes(stored);
    if (!bytes) return null;

    for (const [id, wrappedKey] of decode(bytes) as Array<[Uint8Array, Uint8Array]>) {
      if (bytesToUuid(id) === userId) {
        return encodeBase64(wrappedKey) ?? null;
      }
    }
    return null;
  } catch {
    return null;
  }
}
