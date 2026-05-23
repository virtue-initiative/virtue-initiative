import { describe, expect, it } from 'vitest';
import {
  decryptBatch,
  decompressGzip,
  encryptData,
  encryptForPublicKey,
  generateUserKeyPair,
  unwrapBatchKey,
  verifyBatch,
} from './crypto';

async function makeAesKey(usage: 'encrypt' | 'decrypt' | 'both' = 'both') {
  const usages: KeyUsage[] = usage === 'both' ? ['encrypt', 'decrypt'] : [usage];
  return crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, true, usages);
}

describe('encryptData / decryptBatch', () => {
  it('round-trips arbitrary bytes', async () => {
    const key = await makeAesKey();
    const original = new Uint8Array([1, 2, 3, 4, 5, 100, 200, 255]);
    const encrypted = await encryptData(key, original);
    const decrypted = await decryptBatch(key, encrypted);
    expect(Array.from(decrypted)).toEqual(Array.from(original));
  });

  it('encrypted output starts with 12-byte nonce', async () => {
    const key = await makeAesKey();
    const data = new Uint8Array(16).fill(0xab);
    const encrypted = await encryptData(key, data);
    // nonce (12) + ciphertext + tag (16) = at least 28 bytes + data length
    expect(encrypted.byteLength).toBeGreaterThan(12 + data.length);
  });

  it('each encryption produces different ciphertext (random nonce)', async () => {
    const key = await makeAesKey();
    const data = new Uint8Array(8).fill(0x42);
    const enc1 = await encryptData(key, data);
    const enc2 = await encryptData(key, data);
    expect(Array.from(enc1)).not.toEqual(Array.from(enc2));
  });

  it('decryption fails with wrong key', async () => {
    const key1 = await makeAesKey();
    const key2 = await makeAesKey();
    const data = new Uint8Array([10, 20, 30]);
    const encrypted = await encryptData(key1, data);
    await expect(decryptBatch(key2, encrypted)).rejects.toThrow();
  });
});

describe('generateUserKeyPair / encryptForPublicKey / unwrapBatchKey', () => {
  it('wraps and unwraps a batch key round-trip', async () => {
    const { publicKey, privateKeyHandle } = await generateUserKeyPair();

    // Create an AES key to wrap
    const batchKey = await makeAesKey('decrypt');
    const batchKeyRaw = await crypto.subtle.exportKey('raw', batchKey);

    // Encrypt the raw key material for the public key
    const wrapped = await encryptForPublicKey(publicKey, batchKeyRaw);

    // Unwrap using private key
    const unwrapped = await unwrapBatchKey(privateKeyHandle, wrapped);

    // Verify the unwrapped key works for decryption
    const testData = new Uint8Array([1, 2, 3]);
    const encKey = await crypto.subtle.importKey(
      'raw',
      batchKeyRaw,
      { name: 'AES-GCM', length: 256 },
      false,
      ['encrypt'],
    );
    const encrypted = await encryptData(encKey, testData);
    const decrypted = await decryptBatch(unwrapped, encrypted);
    expect(Array.from(decrypted)).toEqual(Array.from(testData));
  });
});

describe('verifyBatch', () => {
  const zeros = '0'.repeat(64);

  it('returns "verified" when both hashes are zeros and events list is empty', async () => {
    const result = await verifyBatch([], zeros, zeros);
    expect(result).toBe('verified');
  });

  it('returns "verified" for an empty event list with matching hash chain', async () => {
    // Compute expected end hash for empty list starting from a known state
    const startHex = 'a'.repeat(64);
    // For empty events, end hash should equal start hash
    const result = await verifyBatch([], startHex, startHex);
    expect(result).toBe('verified');
  });

  it('returns "failed" when end hash does not match', async () => {
    const startHex = 'a'.repeat(64);
    const wrongEnd = 'b'.repeat(64);
    const result = await verifyBatch([], startHex, wrongEnd);
    expect(result).toBe('failed');
  });

  it('advances hash chain correctly for one event', async () => {
    const startHex = '00'.repeat(32);
    const event = new Uint8Array([0x01, 0x02, 0x03]);

    // Compute expected end state manually
    const startBytes = new Uint8Array(32);
    const contentHash = new Uint8Array(await crypto.subtle.digest('SHA-256', event));
    const buf = new Uint8Array(64);
    buf.set(startBytes, 0);
    buf.set(contentHash, 32);
    const expectedState = new Uint8Array(await crypto.subtle.digest('SHA-256', buf));
    const expectedHex = Array.from(expectedState)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');

    const result = await verifyBatch([event], startHex, expectedHex);
    expect(result).toBe('verified');
  });
});

describe('decompressGzip', () => {
  it('decompresses gzip-compressed data', async () => {
    const original = new TextEncoder().encode('hello, world!');

    // Compress using CompressionStream
    const cs = new CompressionStream('gzip');
    const writer = cs.writable.getWriter();
    await writer.write(original);
    await writer.close();
    const chunks: Uint8Array[] = [];
    const reader = cs.readable.getReader();
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
    }
    const compressed = new Uint8Array(chunks.reduce((s, c) => s + c.length, 0));
    let offset = 0;
    for (const chunk of chunks) {
      compressed.set(chunk, offset);
      offset += chunk.length;
    }

    const decompressed = await decompressGzip(compressed);
    expect(new TextDecoder().decode(decompressed)).toBe('hello, world!');
  });
});
