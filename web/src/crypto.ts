import { argon2id } from "hash-wasm";

// Hashes the password with argon2id before sending to the server.
// Uses lowercased email as a deterministic salt so login is reproducible.
// NOTE: the original (unhashed) password must still be used for the wrapping key.
export async function hashPasswordForAuth(
  password: string,
  email: string,
): Promise<string> {
  return argon2id({
    password,
    salt: email.toLowerCase(),
    iterations: 3,
    memorySize: 65536,
    hashLength: 32,
    parallelism: 1,
    outputType: "hex",
  });
}

export function generateRandomKeyBytes(length = 32): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(new ArrayBuffer(length));
  crypto.getRandomValues(bytes);
  return bytes;
}

// Encrypts data with AES-GCM: returns nonce(12 bytes) || ciphertext+tag
export async function encryptData(
  key: CryptoKey,
  data: Uint8Array,
): Promise<Uint8Array> {
  const payload = Uint8Array.from(data);
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv: nonce },
    key,
    payload,
  );
  const result = new Uint8Array(12 + ciphertext.byteLength);
  result.set(nonce, 0);
  result.set(new Uint8Array(ciphertext), 12);
  return result;
}

// Derives a 256-bit AES-GCM wrapping key (encrypt+decrypt) from password+userId
// Same PBKDF2 parameters as deriveKey
export async function deriveWrappingKey(
  password: string,
  userId: string,
): Promise<CryptoKey> {
  const enc = new TextEncoder();
  const baseKey = await crypto.subtle.importKey(
    "raw",
    enc.encode(password),
    "PBKDF2",
    false,
    ["deriveKey"],
  );
  return crypto.subtle.deriveKey(
    {
      name: "PBKDF2",
      salt: enc.encode(userId),
      iterations: 100_000,
      hash: "SHA-256",
    },
    baseKey,
    { name: "AES-GCM", length: 256 },
    true,
    ["encrypt", "decrypt"],
  );
}

// Derives a 256-bit AES-GCM key from password using PBKDF2-HMAC-SHA256
// salt = UTF-8 bytes of userId, 100_000 iterations
export async function deriveKey(
  password: string,
  userId: string,
  extractable = false,
): Promise<CryptoKey> {
  const enc = new TextEncoder();
  const baseKey = await crypto.subtle.importKey(
    "raw",
    enc.encode(password),
    "PBKDF2",
    false,
    ["deriveKey"],
  );
  return crypto.subtle.deriveKey(
    {
      name: "PBKDF2",
      salt: enc.encode(userId),
      iterations: 100_000,
      hash: "SHA-256",
    },
    baseKey,
    { name: "AES-GCM", length: 256 },
    extractable,
    ["decrypt"],
  );
}

// Decrypts AES-256-GCM blob: first 12 bytes = nonce, rest = ciphertext+tag
export async function decryptBatch(
  key: CryptoKey,
  data: Uint8Array,
): Promise<Uint8Array<ArrayBuffer>> {
  const nonce = data.slice(0, 12);
  const ciphertext = data.slice(12);
  const plain = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv: nonce },
    key,
    ciphertext,
  );
  return new Uint8Array(plain);
}

export async function generateSharingKeyPair(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey(
    {
      name: "RSA-OAEP",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["encrypt", "decrypt"],
  );
}

export async function exportPublicKey(
  key: CryptoKey,
): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await crypto.subtle.exportKey("spki", key));
}

export async function exportPrivateKey(
  key: CryptoKey,
): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(await crypto.subtle.exportKey("pkcs8", key));
}

export async function importPublicKey(spki: BufferSource): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "spki",
    spki,
    { name: "RSA-OAEP", hash: "SHA-256" },
    false,
    ["encrypt"],
  );
}

export async function importPrivateKey(
  pkcs8: BufferSource,
): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "pkcs8",
    pkcs8,
    { name: "RSA-OAEP", hash: "SHA-256" },
    false,
    ["decrypt"],
  );
}

export async function encryptForPublicKey(
  spki: BufferSource,
  data: BufferSource,
): Promise<Uint8Array<ArrayBuffer>> {
  const publicKey = await importPublicKey(spki);
  return new Uint8Array(
    await crypto.subtle.encrypt({ name: "RSA-OAEP" }, publicKey, data),
  );
}

export async function decryptWithPrivateKey(
  privateKey: CryptoKey,
  data: BufferSource,
): Promise<Uint8Array<ArrayBuffer>> {
  return new Uint8Array(
    await crypto.subtle.decrypt({ name: "RSA-OAEP" }, privateKey, data),
  );
}

// Decompresses gzip using native DecompressionStream
export async function decompressGzip(data: Uint8Array): Promise<Uint8Array> {
  const ds = new DecompressionStream("gzip");

  const chunks: Uint8Array[] = [];
  const readPromise = (async () => {
    const reader = ds.readable.getReader();
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
    }
  })();

  const writer = ds.writable.getWriter();
  await writer.write(Uint8Array.from(data));
  await writer.close();
  await readPromise;

  const totalLength = chunks.reduce((s, c) => s + c.length, 0);
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
  const hash = await crypto.subtle.digest("SHA-256", buf);
  return new Uint8Array(hash);
}

const ZEROS_HEX = "0".repeat(64);

export type BatchVerification = "verified" | "failed" | "unknown";

type BatchLogValue = string | number | boolean | Uint8Array;

function appendBatchLogValue(parts: Uint8Array[], value: BatchLogValue) {
  if (value instanceof Uint8Array) {
    parts.push(value);
    return;
  }

  if (typeof value === "number") {
    const bytes = new Uint8Array(8);
    const dv = new DataView(bytes.buffer);
    const lo = value >>> 0;
    const hi = Math.floor(value / 0x100000000);
    dv.setUint32(0, lo, true);
    dv.setUint32(4, hi, true);
    parts.push(bytes);
    return;
  }

  if (typeof value === "boolean") {
    parts.push(new Uint8Array([Number(value)]));
    return;
  }

  parts.push(new TextEncoder().encode(value));
}

/**
 * Verify the hash chain for a batch.
 *
 * Every item advances the chain (including missed captures):
 *   new_state = sha256(current_state || sha256(ts_le[8] || type_utf8 || data_key || data_value || ...))
 *
 * If the final state matches end_chain_hash the batch is verified.
 * Returns 'unknown' when the server has no state tracking (both hashes are zeros).
 */
export async function verifyBatch(
  events: {
    ts: number;
    type: string;
    data: Record<string, BatchLogValue>;
  }[],
  startChainHash: string,
  endChainHash: string,
): Promise<BatchVerification> {
  if (startChainHash === ZEROS_HEX && endChainHash === ZEROS_HEX)
    return "failed";

  const sortedEvents = [...events].sort((a, b) => a.ts - b.ts);

  // Convert startChainHash hex to bytes
  const startBytes = new Uint8Array(32);
  for (let i = 0; i < 32; i++) {
    startBytes[i] = parseInt(startChainHash.slice(i * 2, i * 2 + 2), 16);
  }

  const enc = new TextEncoder();

  let state: Uint8Array = startBytes;
  for (const event of sortedEvents) {
    // Replicate BatchItem::sha256():
    //   ts_le[8] || type_utf8 || data_key || data_value ...
    const tsBytes = new Uint8Array(8);
    const dv = new DataView(tsBytes.buffer);
    const lo = event.ts >>> 0;
    const hi = Math.floor(event.ts / 0x100000000);
    dv.setUint32(0, lo, true);
    dv.setUint32(4, hi, true);

    const typeBytes = enc.encode(event.type);
    const dataParts: Uint8Array[] = [];
    for (const [key, value] of Object.entries(event.data).sort(([a], [b]) =>
      a.localeCompare(b),
    )) {
      dataParts.push(enc.encode(key));
      appendBatchLogValue(dataParts, value);
    }

    const totalLen =
      tsBytes.length +
      typeBytes.length +
      dataParts.reduce((s, p) => s + p.length, 0);
    const buf = new Uint8Array(totalLen);
    let off = 0;
    for (const part of [tsBytes, typeBytes, ...dataParts]) {
      buf.set(part, off);
      off += part.length;
    }

    const contentHash = new Uint8Array(
      await crypto.subtle.digest("SHA-256", buf),
    );
    state = await computeNewState(state, contentHash);
  }

  const computedHex = Array.from(state)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  return computedHex === endChainHash ? "verified" : "failed";
}
