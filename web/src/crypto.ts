import { Aes256Gcm, CipherSuite, HkdfSha256 } from "@hpke/core";
import { DhkemX25519HkdfSha256 } from "@hpke/dhkem-x25519";
import { argon2id } from "hash-wasm";
import type { HashParams } from "./api";

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
    return Uint8Array.from(
      new Uint8Array(value.buffer, value.byteOffset, value.byteLength),
    );
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
  const keyMaterial = await crypto.subtle.importKey(
    "raw",
    rawKey,
    "HKDF",
    false,
    ["deriveBits"],
  );
  const bits = await crypto.subtle.deriveBits(
    {
      name: "HKDF",
      hash: "SHA-256",
      salt: new Uint8Array(),
      info: textEncoder.encode(label),
    },
    keyMaterial,
    256,
  );
  return new Uint8Array(bits);
}

async function importAesKey(
  rawKey: BufferSource,
  usages: KeyUsage[],
  extractable = false,
) {
  return crypto.subtle.importKey(
    "raw",
    rawKey,
    { name: "AES-GCM", length: 256 },
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
    outputType: "binary",
  });
  const passwordAuth = await hkdfSha256("auth", argonOutput);
  const wrappingKeyBytes = await hkdfSha256("key", argonOutput);
  const wrappingKey = await importAesKey(
    wrappingKeyBytes,
    ["encrypt", "decrypt"],
    true,
  );

  return {
    argonOutput,
    passwordAuth,
    wrappingKey,
  };
}

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
  return concatBytes(nonce, new Uint8Array(ciphertext));
}

// Wire format must match client/core/src/crypto.rs: nonce[12 bytes] || ciphertext+tag
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

export async function generateUserKeyPair() {
  const keyPair = await HPKE_SUITE.kem.generateKeyPair();
  return {
    publicKey: new Uint8Array(
      await HPKE_SUITE.kem.serializePublicKey(keyPair.publicKey),
    ),
    privateKey: new Uint8Array(
      await HPKE_SUITE.kem.serializePrivateKey(keyPair.privateKey),
    ),
    privateKeyHandle: keyPair.privateKey,
  };
}

export async function importUserPrivateKey(
  privateKeyBytes: BufferSource,
): Promise<CryptoKey> {
  return HPKE_SUITE.kem.deserializePrivateKey(privateKeyBytes);
}

export async function unwrapBatchKey(
  privateKey: CryptoKey,
  encryptedKey: BufferSource,
) {
  const envelope = toUint8Array(encryptedKey);
  const enc = envelope.slice(0, HPKE_SUITE.kem.encSize);
  const ct = envelope.slice(HPKE_SUITE.kem.encSize);
  const rawKey = new Uint8Array(
    await HPKE_SUITE.open({ recipientKey: privateKey, enc }, ct),
  );

  return importAesKey(rawKey, ["decrypt"]);
}

export async function encryptForPublicKey(
  publicKeyBytes: BufferSource,
  data: BufferSource,
): Promise<Uint8Array<ArrayBuffer>> {
  const recipientPublicKey =
    await HPKE_SUITE.kem.deserializePublicKey(publicKeyBytes);
  const { enc, ct } = await HPKE_SUITE.seal(
    { recipientPublicKey },
    toUint8Array(data),
  );
  return concatBytes(new Uint8Array(enc), new Uint8Array(ct));
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

async function sha256Bytes(
  value: ArrayBufferLike | ArrayBufferView,
): Promise<Uint8Array> {
  const normalized = toUint8Array(value);
  const input = new Uint8Array(normalized.byteLength);
  input.set(normalized);
  return new Uint8Array(await crypto.subtle.digest("SHA-256", input));
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
  if (startChainHash === ZEROS_HEX && endChainHash === ZEROS_HEX)
    return "unknown";

  // Convert startChainHash hex to bytes
  const startBytes = new Uint8Array(32);
  for (let i = 0; i < 32; i++) {
    startBytes[i] = parseInt(startChainHash.slice(i * 2, i * 2 + 2), 16);
  }

  let state: Uint8Array = startBytes;
  for (const event of events) {
    const contentHash = await sha256Bytes(event);
    state = await computeNewState(state, contentHash);
  }

  const computedHex = Array.from(state)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  return computedHex === endChainHash ? "verified" : "failed";
}
