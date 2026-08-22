import { api, setUnauthorizedHandler, User } from './api';
import { cacheClient } from '../cache/client';
import {
  decryptBatch,
  derivePasswordMaterial,
  encryptData,
  generateRandomKeyBytes,
  generateUserKeyPair,
  importUserPrivateKey,
} from './crypto';

const WRAPPING_KEY_STORAGE = 'virtue_wrapping_key';

async function saveWrappingKey(wk: CryptoKey): Promise<void> {
  const raw = await crypto.subtle.exportKey('raw', wk);
  localStorage.setItem(WRAPPING_KEY_STORAGE, btoa(String.fromCharCode(...new Uint8Array(raw))));
}

async function loadWrappingKey(): Promise<CryptoKey | null> {
  const stored = localStorage.getItem(WRAPPING_KEY_STORAGE);
  if (!stored) return null;
  try {
    const raw = Uint8Array.from(atob(stored), (c) => c.charCodeAt(0));
    return crypto.subtle.importKey('raw', raw, { name: 'AES-GCM', length: 256 }, true, [
      'encrypt',
      'decrypt',
    ]);
  } catch {
    return null;
  }
}

function clearWrappingKey(): void {
  localStorage.removeItem(WRAPPING_KEY_STORAGE);
}

export class Session {
  userId: string;
  wrappingKey: CryptoKey;
  privateKey: CryptoKey | null;
  private invalidated = false;
  private onInvalidate: (() => void) | null = null;

  private constructor(userId: string, wrappingKey: CryptoKey, privateKey: CryptoKey | null) {
    this.userId = userId;
    this.wrappingKey = wrappingKey;
    this.privateKey = privateKey;
  }

  static async fromLogin(email: string, password: string): Promise<Session> {
    const material = await api.getLoginMaterial(email);
    const { passwordAuth, wrappingKey } = await derivePasswordMaterial(
      password,
      Uint8Array.fromBase64(material.password_salt),
      material.params,
    );
    await api.login(
      email,
      passwordAuth.toBase64(),
      Intl.DateTimeFormat().resolvedOptions().timeZone,
    );
    await saveWrappingKey(wrappingKey);
    const user = await api.getUser();
    const privateKey = await decryptStoredPrivateKey(user, wrappingKey);
    const session = new Session(user.id, wrappingKey, privateKey);
    session.installUnauthorizedHandler();
    return session;
  }

  static async fromFinishSignup(
    token: string,
    name: string | undefined,
    password: string,
  ): Promise<Session> {
    const params = await api.getCurrentHashParams();
    const passwordSalt = generateRandomKeyBytes(params.salt_length);
    const { passwordAuth, wrappingKey } = await derivePasswordMaterial(
      password,
      passwordSalt,
      params,
    );
    const keyPair = await generateUserKeyPair();
    const encryptedPrivateKey = await encryptData(wrappingKey, keyPair.privateKey);
    const res = await api.signup({
      verification_token: token,
      password_auth: passwordAuth.toBase64(),
      password_salt: passwordSalt.toBase64(),
      pub_key: keyPair.publicKey.toBase64(),
      encrypted_priv_key: encryptedPrivateKey.toBase64(),
      ...(name ? { name } : {}),
    });
    await saveWrappingKey(wrappingKey);
    const privateKey = await importUserPrivateKey(keyPair.privateKey);
    const session = new Session(res.user.id, wrappingKey, privateKey);
    session.installUnauthorizedHandler();
    return session;
  }

  static async restore(): Promise<Session | null> {
    const wrappingKey = await loadWrappingKey().catch(() => null);
    if (!wrappingKey) return null;
    let user: User;
    try {
      user = await api.getUser();
    } catch {
      return null;
    }
    const privateKey = await decryptStoredPrivateKey(user, wrappingKey);
    const session = new Session(user.id, wrappingKey, privateKey);
    session.installUnauthorizedHandler();
    return session;
  }

  onTokenRefreshFailed(cb: () => void) {
    this.onInvalidate = cb;
  }

  isInvalidated(): boolean {
    return this.invalidated;
  }

  async logout(): Promise<void> {
    await api.logout().catch(() => {});
    await this.invalidate();
  }

  async unwrapPrivateKey(encryptedPrivKey: string): Promise<void> {
    const rawPrivateKey = await decryptBatch(
      this.wrappingKey,
      Uint8Array.fromBase64(encryptedPrivKey),
    );
    this.privateKey = await importUserPrivateKey(rawPrivateKey);
  }

  private async invalidate(): Promise<void> {
    this.invalidated = true;
    setUnauthorizedHandler(null);
    await cacheClient?.clearCache().catch(() => {});
    clearWrappingKey();
    this.onInvalidate?.();
  }

  private installUnauthorizedHandler() {
    setUnauthorizedHandler(() => {
      void this.invalidate();
    });
  }
}

async function decryptStoredPrivateKey(
  user: User,
  wrappingKey: CryptoKey,
): Promise<CryptoKey | null> {
  if (!user.encrypted_priv_key) return null;
  try {
    const raw = await decryptBatch(wrappingKey, Uint8Array.fromBase64(user.encrypted_priv_key));
    return await importUserPrivateKey(raw);
  } catch (err) {
    console.error('Failed to restore private key', err);
    return null;
  }
}
