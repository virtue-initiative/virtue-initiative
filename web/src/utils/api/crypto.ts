import { Aes256Gcm, CipherSuite, HkdfSha256 } from '@hpke/core';
import { DhkemX25519HkdfSha256 } from '@hpke/dhkem-x25519';
import { argon2id, createSHA256, type IHasher } from 'hash-wasm';
import type { HashParams } from './api';

const textEncoder = new TextEncoder();
const HPKE_SUITE = new CipherSuite({
  kem: new DhkemX25519HkdfSha256(),
  kdf: new HkdfSha256(),
  aead: new Aes256Gcm(),
});

function toUint8Array(value: ArrayBufferLike | ArrayBufferView): Uint8Array {
  if (value instanceof Uint8Array) {
    return Uint8Array.from(value);
  }

  if (ArrayBuffer.isView(value)) {
    return Uint8Array.from(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
  }

  return Uint8Array.from(new Uint8Array(value));
}

function concatBytes(...parts: Uint8Array[]) {
  const totalLength = parts.reduce((sum, part) => sum + part.length, 0);
  const output = new Uint8Array(totalLength);
  let offset = 0;

  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }

  return output;
}

async function hkdfSha256(label: string, ikm: Uint8Array) {
  const rawKey = new Uint8Array(ikm.length);
  rawKey.set(ikm);
  const keyMaterial = await crypto.subtle.importKey('raw', rawKey, 'HKDF', false, ['deriveBits']);
  const bits = await crypto.subtle.deriveBits(
    {
      name: 'HKDF',
      hash: 'SHA-256',
      salt: new Uint8Array(),
      info: textEncoder.encode(label),
    },
    keyMaterial,
    256,
  );
  return new Uint8Array(bits);
}

async function importAesKey(rawKey: BufferSource, usages: KeyUsage[], extractable = false) {
  return crypto.subtle.importKey(
    'raw',
    rawKey,
    { name: 'AES-GCM', length: 256 },
    extractable,
    usages,
  );
}

export function generateRandomKeyBytes(length = 32): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(new ArrayBuffer(length));
  crypto.getRandomValues(bytes);
  return bytes;
}

// Must match client/core/src/api.rs: argon2id(password, salt=lowercase_email, m=65536, t=3, p=1, len=32)
// then HKDF-SHA256("auth"|"key", argon_output). Changing parameters breaks login for existing accounts.
export async function derivePasswordMaterial(
  password: string,
  passwordSalt: Uint8Array,
  params: HashParams,
) {
  const argonOutput = await argon2id({
    password,
    salt: passwordSalt,
    iterations: params.time_cost,
    memorySize: params.memory_cost_kib,
    hashLength: 32,
    parallelism: params.parallelism,
    outputType: 'binary',
  });
  const passwordAuth = await hkdfSha256('auth', argonOutput);
  const wrappingKeyBytes = await hkdfSha256('key', argonOutput);
  const wrappingKey = await importAesKey(wrappingKeyBytes, ['encrypt', 'decrypt'], true);

  return {
    argonOutput,
    passwordAuth,
    wrappingKey,
  };
}

export async function encryptData(key: CryptoKey, data: Uint8Array): Promise<Uint8Array> {
  const payload = Uint8Array.from(data);
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce }, key, payload);
  return concatBytes(nonce, new Uint8Array(ciphertext));
}

// Wire format must match client/core/src/crypto.rs: nonce[12 bytes] || ciphertext+tag
export async function decryptBatch(
  key: CryptoKey,
  data: Uint8Array,
): Promise<Uint8Array<ArrayBuffer>> {
  const nonce = data.slice(0, 12);
  const ciphertext = data.slice(12);
  const plain = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: nonce }, key, ciphertext);
  return new Uint8Array(plain);
}

export async function generateUserKeyPair() {
  const keyPair = await HPKE_SUITE.kem.generateKeyPair();
  return {
    publicKey: new Uint8Array(await HPKE_SUITE.kem.serializePublicKey(keyPair.publicKey)),
    privateKey: new Uint8Array(await HPKE_SUITE.kem.serializePrivateKey(keyPair.privateKey)),
    privateKeyHandle: keyPair.privateKey,
  };
}

export async function importUserPrivateKey(privateKeyBytes: BufferSource): Promise<CryptoKey> {
  return HPKE_SUITE.kem.deserializePrivateKey(privateKeyBytes);
}

// Raw 32-byte X25519 private scalar for the user's HPKE key. Used by the native-WebCrypto
// unwrap path (hpke-native.ts) to import the key into crypto.subtle.
export async function exportUserPrivateKey(privateKey: CryptoKey): Promise<Uint8Array> {
  return new Uint8Array(await HPKE_SUITE.kem.serializePrivateKey(privateKey));
}

export async function unwrapBatchKey(privateKey: CryptoKey, encryptedKey: BufferSource) {
  const envelope = toUint8Array(encryptedKey);
  const enc = envelope.slice(0, HPKE_SUITE.kem.encSize);
  const ct = envelope.slice(HPKE_SUITE.kem.encSize);
  const rawKey = new Uint8Array(await HPKE_SUITE.open({ recipientKey: privateKey, enc }, ct));

  return importAesKey(rawKey, ['decrypt']);
}

// Same envelope as unwrapBatchKey, but returns the opened plaintext bytes
// directly instead of importing them as an AES key -- used for locked
// passwords, which are sealed once for the owner's own pub_key rather than
// wrapping a symmetric batch key.
export async function decryptForOwnKey(
  privateKey: CryptoKey,
  wrapped: BufferSource,
): Promise<Uint8Array> {
  const envelope = toUint8Array(wrapped);
  const enc = envelope.slice(0, HPKE_SUITE.kem.encSize);
  const ct = envelope.slice(HPKE_SUITE.kem.encSize);
  return new Uint8Array(await HPKE_SUITE.open({ recipientKey: privateKey, enc }, ct));
}

export async function encryptForPublicKey(
  publicKeyBytes: BufferSource,
  data: BufferSource,
): Promise<Uint8Array<ArrayBuffer>> {
  const recipientPublicKey = await HPKE_SUITE.kem.deserializePublicKey(publicKeyBytes);
  const { enc, ct } = await HPKE_SUITE.seal({ recipientPublicKey }, toUint8Array(data));
  return concatBytes(new Uint8Array(enc), new Uint8Array(ct));
}

// Decompresses gzip using native DecompressionStream
export async function decompressGzip(data: Uint8Array): Promise<Uint8Array> {
  const ds = new DecompressionStream('gzip');

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

export type BatchVerification = 'verified' | 'failed' | 'unknown';

// Lazily-created synchronous SHA-256 hasher (hash-wasm). We use this instead of the async
// crypto.subtle.digest for the hash chain because verifyBatch issues two digests per event,
// chained sequentially — thousands of awaited subtle calls dominated the decrypt pipeline.
// A synchronous WASM hasher computes the whole chain inline with zero promise/dispatch
// overhead. Output bytes are identical (plain SHA-256), so the cross-component hash-chain
// contract is unchanged.
let sha256HasherPromise: Promise<IHasher> | null = null;
function getSha256Hasher(): Promise<IHasher> {
  if (!sha256HasherPromise) sha256HasherPromise = createSHA256();
  return sha256HasherPromise;
}

/**
 * Verify the hash chain for a batch.
 *
 * Every item advances the chain:
 *   new_state = sha256(current_state || sha256(raw_event_msgpack_bytes))
 *
 * If the final state matches end_chain_hash the batch is verified.
 * Returns 'unknown' when the server has no state tracking (both hashes are zeros).
 */
export async function verifyBatch(
  events: Array<ArrayBufferLike | ArrayBufferView>,
  startChainHash: string,
  endChainHash: string,
): Promise<BatchVerification> {
  const hasher = await getSha256Hasher();

  // Convert startChainHash hex to bytes
  let state = new Uint8Array(32);
  for (let i = 0; i < 32; i++) {
    state[i] = parseInt(startChainHash.slice(i * 2, i * 2 + 2), 16);
  }

  const buf = new Uint8Array(64);
  for (const event of events) {
    // contentHash = sha256(event)
    hasher.init();
    hasher.update(toUint8Array(event));
    const contentHash = hasher.digest('binary');

    // state = sha256(state || contentHash). Copy both into buf before re-using the hasher
    // so we never read a digest buffer the next hasher call might overwrite.
    buf.set(state, 0);
    buf.set(contentHash, 32);
    hasher.init();
    hasher.update(buf);
    state = hasher.digest('binary').slice();
  }

  const computedHex = Array.from(state)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
  return computedHex === endChainHash ? 'verified' : 'failed';
}
