// Deterministic account material for seeded users: the password hash the API
// checks at login, plus the HPKE key pair the web app unwraps batch keys with.
//
// Password derivation reuses derivePasswordMaterial from the web app so the
// seeded hash can't drift from what the login form sends (CLAUDE.md contract 4).
import { DhkemX25519HkdfSha256 } from '@hpke/dhkem-x25519';
import { CURRENT_HASH_PARAMS } from '../../api/src/lib/password';
import { derivePasswordMaterial } from '../../web/src/utils/api/crypto';
import { toHex } from './util';

export const PASSWORD = 'devpassword';

export interface SeedUser {
  // 32 hex chars. Must be a valid v4 UUID (version nibble 4, variant 8/9/a/b)
  // or z.uuid() rejects it as an access_keys key on batch upload.
  idHex: string;
  email: string;
  name: string | null;
  // Fixed so every run produces the same salt and key pair.
  salt: Uint8Array<ArrayBuffer>;
  keyIkm: Uint8Array<ArrayBuffer>;
  // Fixed nonce for encrypting the private key, again for determinism.
  keyNonce: Uint8Array<ArrayBuffer>;
}

export interface AccountMaterial {
  passwordHashHex: string;
  saltHex: string;
  pubKey: Uint8Array;
  pubKeyHex: string;
  encryptedPrivKeyHex: string;
}

export async function deriveAccount(user: SeedUser): Promise<AccountMaterial> {
  const { passwordAuth, wrappingKey } = await derivePasswordMaterial(
    PASSWORD,
    user.salt,
    CURRENT_HASH_PARAMS,
  );
  const passwordHash = new Uint8Array(await crypto.subtle.digest('SHA-256', passwordAuth));

  const kem = new DhkemX25519HkdfSha256();
  const keyPair = await kem.deriveKeyPair(user.keyIkm.buffer);
  const pubKey = new Uint8Array(await kem.serializePublicKey(keyPair.publicKey));
  const privKey = new Uint8Array(await kem.serializePrivateKey(keyPair.privateKey));

  // nonce || ciphertext, same layout as encryptData in web/src/utils/api/crypto.ts
  const sealed = new Uint8Array(
    await crypto.subtle.encrypt({ name: 'AES-GCM', iv: user.keyNonce }, wrappingKey, privKey),
  );

  return {
    passwordHashHex: toHex(passwordHash),
    saltHex: toHex(user.salt),
    pubKey,
    pubKeyHex: toHex(pubKey),
    encryptedPrivKeyHex: toHex(user.keyNonce) + toHex(sealed),
  };
}
