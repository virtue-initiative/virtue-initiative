// Native-WebCrypto implementation of the HPKE recipient open used to unwrap batch keys.
//
// WHY THIS EXISTS
// `@hpke/dhkem-x25519` implements the X25519 Diffie-Hellman in pure JavaScript (a noble-curves
// Montgomery ladder), which runs synchronously on the worker thread and is the dominant
// serialized cost of a large decrypt run. This module reimplements the recipient side of
// HPKE base-mode `open()` for the exact suite we use — DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256
// / AES-256-GCM — using crypto.subtle for the X25519 DH, all HKDF/HMAC steps, and the AEAD.
// That moves the work off-thread onto the browser's native crypto pool.
//
// CORRECTNESS
// This MUST stay byte-for-byte compatible with `@hpke/core` (and therefore the Rust client).
// hpke-native.test.ts verifies that a key wrapped by `encryptForPublicKey` (which uses
// `@hpke`) unwraps to the identical batch key here. Do not change labels, suite ids, or the
// key-schedule order without re-running that test.
//
// Spec references: RFC 9180 §4.1 (DHKEM), §5.1 (key schedule), §7.1 (suite ids).

import { exportUserPrivateKey } from './crypto';

const te = new TextEncoder();

// Suite ids (RFC 9180 §7.1). KemId.DhkemX25519HkdfSha256 = 0x0020,
// KdfId.HkdfSha256 = 0x0001, AeadId.Aes256Gcm = 0x0002.
const SUITE_KEM = concat(te.encode('KEM'), Uint8Array.of(0x00, 0x20));
const SUITE_HPKE = concat(te.encode('HPKE'), Uint8Array.of(0x00, 0x20, 0x00, 0x01, 0x00, 0x02));
const HPKE_V1 = te.encode('HPKE-v1');
const EMPTY = new Uint8Array(0);

// X25519 base point u=9 (little-endian), used to derive the recipient public key from the
// private scalar via a single native scalar-mult.
const X25519_BASE_POINT = (() => {
  const b = new Uint8Array(32);
  b[0] = 9;
  return b;
})();

// PKCS#8 prefix for an X25519 private key (OID 1.3.101.110), followed by the 32 raw bytes.
const PKCS8_X25519_PREFIX = Uint8Array.of(
  0x30,
  0x2e,
  0x02,
  0x01,
  0x00,
  0x30,
  0x05,
  0x06,
  0x03,
  0x2b,
  0x65,
  0x6e,
  0x04,
  0x22,
  0x04,
  0x20,
);

// Byte buffers passed to crypto.subtle must be ArrayBuffer-backed (not SharedArrayBuffer),
// hence the explicit Uint8Array<ArrayBuffer> throughout.
type Bytes = Uint8Array<ArrayBuffer>;

function concat(...parts: Uint8Array[]): Bytes {
  let len = 0;
  for (const p of parts) len += p.length;
  const out = new Uint8Array(len);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

async function hmacSha256(key: Bytes, data: Bytes): Promise<Bytes> {
  const k = await crypto.subtle.importKey('raw', key, { name: 'HMAC', hash: 'SHA-256' }, false, [
    'sign',
  ]);
  return new Uint8Array(await crypto.subtle.sign('HMAC', k, data));
}

// HKDF-Extract (RFC 5869). Empty salt → HashLen (32) zero bytes.
function hkdfExtract(salt: Bytes, ikm: Bytes): Promise<Bytes> {
  return hmacSha256(salt.length ? salt : new Uint8Array(32), ikm);
}

// HKDF-Expand for L ≤ 32 (single block: T(1) = HMAC(prk, info || 0x01)).
async function hkdfExpand(prk: Bytes, info: Bytes, length: number): Promise<Bytes> {
  const t1 = await hmacSha256(prk, concat(info, Uint8Array.of(0x01)));
  return t1.slice(0, length);
}

// RFC 9180 §4.
function labeledExtract(salt: Bytes, label: string, ikm: Bytes, suiteId: Bytes): Promise<Bytes> {
  return hkdfExtract(salt, concat(HPKE_V1, suiteId, te.encode(label), ikm));
}

function labeledExpand(
  prk: Bytes,
  label: string,
  info: Bytes,
  length: number,
  suiteId: Bytes,
): Promise<Bytes> {
  const labeledInfo = concat(
    Uint8Array.of((length >> 8) & 0xff, length & 0xff),
    HPKE_V1,
    suiteId,
    te.encode(label),
    info,
  );
  return hkdfExpand(prk, labeledInfo, length);
}

/** Whether crypto.subtle in this environment supports the X25519 operations we need. */
export async function nativeX25519Supported(): Promise<boolean> {
  try {
    await crypto.subtle.importKey('raw', X25519_BASE_POINT, { name: 'X25519' }, false, []);
    return true;
  } catch {
    return false;
  }
}

/**
 * Build a per-batch unwrapper backed by native WebCrypto, or return null if the environment
 * doesn't support X25519 in crypto.subtle. The returned function takes the wrapped-key envelope
 * (`enc[32] || ciphertext+tag`) and returns the unwrapped AES-256-GCM batch key, matching
 * `unwrapBatchKey` from crypto.ts.
 */
export async function createNativeBatchKeyUnwrapper(
  privateKey: CryptoKey,
): Promise<((encryptedKey: Bytes) => Promise<CryptoKey>) | null> {
  if (!(await nativeX25519Supported())) return null;

  let skKey: CryptoKey;
  let pkRm: Bytes;
  try {
    const skRaw = await exportUserPrivateKey(privateKey);
    skKey = await crypto.subtle.importKey(
      'pkcs8',
      concat(PKCS8_X25519_PREFIX, skRaw),
      { name: 'X25519' },
      false,
      ['deriveBits'],
    );
    const basePub = await crypto.subtle.importKey(
      'raw',
      X25519_BASE_POINT,
      { name: 'X25519' },
      false,
      [],
    );
    pkRm = new Uint8Array(
      await crypto.subtle.deriveBits({ name: 'X25519', public: basePub }, skKey, 256),
    );
  } catch {
    return null;
  }

  // Key-schedule context is constant for base mode with empty psk/info, so precompute it once.
  const pskIdHash = await labeledExtract(EMPTY, 'psk_id_hash', EMPTY, SUITE_HPKE);
  const infoHash = await labeledExtract(EMPTY, 'info_hash', EMPTY, SUITE_HPKE);
  const keyScheduleContext = concat(Uint8Array.of(0x00), pskIdHash, infoHash);

  return async (encryptedKey: Bytes): Promise<CryptoKey> => {
    const enc = encryptedKey.slice(0, 32);
    const ct = encryptedKey.slice(32);

    // DHKEM decap: dh = DH(skR, enc); shared_secret = ExtractAndExpand(dh, enc || pkRm).
    const encPub = await crypto.subtle.importKey('raw', enc, { name: 'X25519' }, false, []);
    const dh = new Uint8Array(
      await crypto.subtle.deriveBits({ name: 'X25519', public: encPub }, skKey, 256),
    );
    const kemContext = concat(enc, pkRm);
    const eaePrk = await labeledExtract(EMPTY, 'eae_prk', dh, SUITE_KEM);
    const sharedSecret = await labeledExpand(eaePrk, 'shared_secret', kemContext, 32, SUITE_KEM);

    // Key schedule: secret → AEAD key + base nonce (seq 0, so nonce = base_nonce).
    const secret = await labeledExtract(sharedSecret, 'secret', EMPTY, SUITE_HPKE);
    const key = await labeledExpand(secret, 'key', keyScheduleContext, 32, SUITE_HPKE);
    const baseNonce = await labeledExpand(secret, 'base_nonce', keyScheduleContext, 12, SUITE_HPKE);

    const aeadKey = await crypto.subtle.importKey('raw', key, { name: 'AES-GCM' }, false, [
      'decrypt',
    ]);
    const rawBatchKey = new Uint8Array(
      await crypto.subtle.decrypt({ name: 'AES-GCM', iv: baseNonce }, aeadKey, ct),
    );
    return crypto.subtle.importKey('raw', rawBatchKey, { name: 'AES-GCM' }, false, ['decrypt']);
  };
}
