import { describe, expect, it } from 'vitest';
import {
  encryptForPublicKey,
  generateUserKeyPair,
  importUserPrivateKey,
  unwrapBatchKey,
} from './crypto';
import { createNativeBatchKeyUnwrapper, nativeX25519Supported } from './hpke-native';

// Decrypt a sample message with an AES-256-GCM key to prove two keys are identical.
async function decryptSampleWith(
  key: CryptoKey,
  nonce: Uint8Array<ArrayBuffer>,
  ct: Uint8Array<ArrayBuffer>,
) {
  return new Uint8Array(await crypto.subtle.decrypt({ name: 'AES-GCM', iv: nonce }, key, ct));
}

describe('hpke-native unwrap', () => {
  it('recovers the same batch key as @hpke for a wrapped key', async (ctx) => {
    if (!(await nativeX25519Supported())) {
      // Environment (e.g. older Node) lacks X25519 in crypto.subtle; nothing to verify here.
      ctx.skip();
      return;
    }

    // A user keypair and a random 32-byte batch key, wrapped to the public key via @hpke.
    const keyPair = await generateUserKeyPair();
    const batchKeyBytes = crypto.getRandomValues(new Uint8Array(32));
    const wrapped = await encryptForPublicKey(keyPair.publicKey, batchKeyBytes);

    const privateKey = await importUserPrivateKey(keyPair.privateKey);
    const nativeUnwrap = await createNativeBatchKeyUnwrapper(privateKey);
    expect(nativeUnwrap).not.toBeNull();

    const nativeKey = await nativeUnwrap!(wrapped);
    const hpkeKey = await unwrapBatchKey(privateKey, wrapped);

    // Encrypt a sample under the original batch key, then decrypt with both unwrapped keys.
    const original = crypto.subtle.importKey('raw', batchKeyBytes, { name: 'AES-GCM' }, false, [
      'encrypt',
    ]);
    const nonce = crypto.getRandomValues(new Uint8Array(12));
    const message = new TextEncoder().encode('the quick brown fox');
    const ct = new Uint8Array(
      await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce }, await original, message),
    );

    const fromNative = await decryptSampleWith(nativeKey, nonce, ct);
    const fromHpke = await decryptSampleWith(hpkeKey, nonce, ct);

    expect(Array.from(fromNative)).toEqual(Array.from(message));
    expect(Array.from(fromHpke)).toEqual(Array.from(message));
  });
});
