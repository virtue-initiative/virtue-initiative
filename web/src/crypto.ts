import { argon2id } from 'hash-wasm';

// Hashes the password with argon2id before sending to the server.
// Uses lowercased email as a deterministic salt so login is reproducible.
// NOTE: the original (unhashed) password must still be used for the wrapping key.
export async function hashPasswordForAuth(password: string, email: string): Promise<string> {
  return argon2id({
    password,
    salt: email.toLowerCase(),
    iterations: 3,
    memorySize: 65536,
    hashLength: 32,
    parallelism: 1,
    outputType: 'hex',
  });
}

// Encrypts data with AES-GCM: returns nonce(12 bytes) || ciphertext+tag
export async function encryptData(key: CryptoKey, data: Uint8Array<ArrayBuffer>): Promise<Uint8Array> {
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce }, key, data);
  const result = new Uint8Array(12 + ciphertext.byteLength);
  result.set(nonce, 0);
  result.set(new Uint8Array(ciphertext), 12);
  return result;
}

// Derives a 256-bit AES-GCM wrapping key (encrypt+decrypt) from password+userId
// Same PBKDF2 parameters as deriveKey
export async function deriveWrappingKey(password: string, userId: string): Promise<CryptoKey> {
  const enc = new TextEncoder();
  const baseKey = await crypto.subtle.importKey('raw', enc.encode(password), 'PBKDF2', false, ['deriveKey']);
  return crypto.subtle.deriveKey(
    { name: 'PBKDF2', salt: enc.encode(userId), iterations: 100_000, hash: 'SHA-256' },
    baseKey,
    { name: 'AES-GCM', length: 256 },
    true,
    ['encrypt', 'decrypt'],
  );
}

// Derives a 256-bit AES-GCM key from password using PBKDF2-HMAC-SHA256
// salt = UTF-8 bytes of userId, 100_000 iterations
export async function deriveKey(password: string, userId: string, extractable = false): Promise<CryptoKey> {
  const enc = new TextEncoder();
  const baseKey = await crypto.subtle.importKey(
    'raw',
    enc.encode(password),
    'PBKDF2',
    false,
    ['deriveKey'],
  );
  return crypto.subtle.deriveKey(
    {
      name: 'PBKDF2',
      salt: enc.encode(userId),
      iterations: 100_000,
      hash: 'SHA-256',
    },
    baseKey,
    { name: 'AES-GCM', length: 256 },
    extractable,
    ['decrypt'],
  );
}

// Decrypts AES-256-GCM blob: first 12 bytes = nonce, rest = ciphertext+tag
export async function decryptBatch(key: CryptoKey, data: Uint8Array): Promise<Uint8Array<ArrayBuffer>> {
  const nonce = data.slice(0, 12);
  const ciphertext = data.slice(12);
  const plain = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: nonce },
    key,
    ciphertext,
  );
  return new Uint8Array(plain);
}

// Decompresses gzip using native DecompressionStream
export async function decompressGzip(data: Uint8Array): Promise<Uint8Array> {
  const ds = new DecompressionStream('gzip');
  const writer = ds.writable.getWriter();
  const reader = ds.readable.getReader();

  const input = Uint8Array.from(data);
  await writer.write(input);
  await writer.close();

  const chunks: Uint8Array[] = [];
  let totalLength = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    totalLength += value.length;
  }

  const result = new Uint8Array(totalLength);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return result;
}

// SHA-256(currentState[32] || contentHash[32])
// currentState: rolling state from the server (or 32 zero bytes initially)
// contentHash:  SHA-256 of the plaintext log item being recorded
export async function computeNewState(
  currentState: Uint8Array,
  contentHash: Uint8Array,
): Promise<Uint8Array> {
  const buf = new Uint8Array(64);
  buf.set(currentState, 0);
  buf.set(contentHash, 32);
  const hash = await crypto.subtle.digest('SHA-256', buf);
  return new Uint8Array(hash);
}

const ZEROS_HEX = '0'.repeat(64);

export type BatchVerification = 'verified' | 'failed' | 'unknown';

/**
 * Verify the hash chain for a batch.
 *
 * Every item advances the chain (including missed captures):
 *   new_state = sha256(current_state || sha256(id[16] || taken_at_le[8] || kind_utf8 || image_bytes || meta_k1 || meta_v1 || ...))
 *
 * If the final state matches end_chain_hash the batch is verified.
 * Returns 'unknown' when the server has no state tracking (both hashes are zeros).
 */
export async function verifyBatch(
  items: { id: string; image?: Uint8Array; taken_at: number; kind: string; metadata: [string, string][] }[],
  startChainHash: string,
  endChainHash: string,
): Promise<BatchVerification> {
  if (startChainHash === ZEROS_HEX && endChainHash === ZEROS_HEX) return 'unknown';

  const sortedItems = [...items].sort((a, b) => a.taken_at - b.taken_at);

  // Convert startChainHash hex to bytes
  const startBytes = new Uint8Array(32);
  for (let i = 0; i < 32; i++) {
    startBytes[i] = parseInt(startChainHash.slice(i * 2, i * 2 + 2), 16);
  }

  const enc = new TextEncoder();

  let state: Uint8Array = startBytes;
  for (const item of sortedItems) {
    // Replicate BatchItem::sha256():
    //   id[16] || taken_at_le[8] || kind_utf8 || image_bytes || meta_k || meta_v ...
    const idHex = item.id.replace(/-/g, '');
    const idBytes = new Uint8Array(16);
    for (let i = 0; i < 16; i++) idBytes[i] = parseInt(idHex.slice(i * 2, i * 2 + 2), 16);

    const takenAtBytes = new Uint8Array(8);
    const dv = new DataView(takenAtBytes.buffer);
    // taken_at is ms epoch as i64 little-endian; JS numbers are safe up to 2^53
    const lo = item.taken_at >>> 0;
    const hi = Math.floor(item.taken_at / 0x100000000);
    dv.setUint32(0, lo, true);
    dv.setUint32(4, hi, true);

    const kindBytes = enc.encode(item.kind);
    const imageBytes = item.image ?? new Uint8Array(0);
    const metaParts: Uint8Array[] = [];
    for (const [k, v] of item.metadata) {
      metaParts.push(enc.encode(k), enc.encode(v));
    }

    const totalLen = idBytes.length + takenAtBytes.length + kindBytes.length + imageBytes.length
      + metaParts.reduce((s, p) => s + p.length, 0);
    const buf = new Uint8Array(totalLen);
    let off = 0;
    for (const part of [idBytes, takenAtBytes, kindBytes, imageBytes, ...metaParts]) {
      buf.set(part, off);
      off += part.length;
    }

    const contentHash = new Uint8Array(await crypto.subtle.digest('SHA-256', buf));
    state = await computeNewState(state, contentHash);
  }

  const computedHex = Array.from(state).map((b) => b.toString(16).padStart(2, '0')).join('');
  return computedHex === endChainHash ? 'verified' : 'failed';
}
