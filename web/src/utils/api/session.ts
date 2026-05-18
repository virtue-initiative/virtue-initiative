import { api, setReauthHandler, User } from './api';
import { clearDataCache } from './data-cache';
import {
  decryptBatch,
  derivePasswordMaterial,
  encryptData,
  generateRandomKeyBytes,
  generateUserKeyPair,
  importUserPrivateKey,
} from './crypto';
import { DEFAULT_DIGEST_LOCAL_HOUR, localHourToUtcMinutes } from '../digest';

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

function jwtSub(t: string): string | null {
  try {
    const b64 = t.split('.')[1].replace(/-/g, '+').replace(/_/g, '/');
    const padded = b64 + '='.repeat((4 - (b64.length % 4)) % 4);
    return JSON.parse(atob(padded)).sub ?? null;
  } catch {
    return null;
  }
}

export class Session {
  token: string;
  userId: string;
  wrappingKey: CryptoKey;
  privateKey: CryptoKey | null;
  private invalidated = false;
  private onInvalidate: (() => void) | null = null;

  private constructor(
    token: string,
    userId: string,
    wrappingKey: CryptoKey,
    privateKey: CryptoKey | null,
  ) {
    this.token = token;
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
    const res = await api.login(email, passwordAuth.toBase64());
    const userId = jwtSub(res.access_token);
    if (!userId) throw new Error('Login token is missing a subject');
    await saveWrappingKey(wrappingKey);
    const user = await api.getUser(res.access_token);
    const privateKey = await decryptStoredPrivateKey(user, wrappingKey);
    const session = new Session(res.access_token, userId, wrappingKey, privateKey);
    session.installReauthHandler();
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
      priv_key: encryptedPrivateKey.toBase64(),
      ...(name ? { name } : {}),
      email_digest_minutes_utc: localHourToUtcMinutes(DEFAULT_DIGEST_LOCAL_HOUR),
    });
    const userId = jwtSub(res.access_token);
    if (!userId) throw new Error('Signup access token is missing a subject');
    await saveWrappingKey(wrappingKey);
    const privateKey = await importUserPrivateKey(keyPair.privateKey);
    const session = new Session(res.access_token, userId, wrappingKey, privateKey);
    session.installReauthHandler();
    return session;
  }

  static async restore(): Promise<Session | null> {
    let access_token: string;
    try {
      const res = await api.refreshToken();
      access_token = res.access_token;
    } catch {
      return null;
    }
    const userId = jwtSub(access_token);
    if (!userId) return null;
    const wrappingKey = await loadWrappingKey().catch(() => null);
    if (!wrappingKey) return null;
    let user: User;
    try {
      user = await api.getUser(access_token);
    } catch {
      return null;
    }
    const privateKey = await decryptStoredPrivateKey(user, wrappingKey);
    const session = new Session(access_token, userId, wrappingKey, privateKey);
    session.installReauthHandler();
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
    setReauthHandler(null);
    await clearDataCache().catch(() => {});
    clearWrappingKey();
    this.onInvalidate?.();
  }

  private installReauthHandler() {
    setReauthHandler(async () => {
      try {
        const res = await api.refreshToken();
        const uid = jwtSub(res.access_token);
        if (!uid) throw new Error('Refreshed token missing subject');
        this.token = res.access_token;
        this.userId = uid;
        return res.access_token;
      } catch {
        await this.invalidate();
        return null;
      }
    });
  }
}

async function decryptStoredPrivateKey(
  user: User,
  wrappingKey: CryptoKey,
): Promise<CryptoKey | null> {
  if (!user.priv_key) return null;
  try {
    const raw = await decryptBatch(wrappingKey, Uint8Array.fromBase64(user.priv_key));
    return await importUserPrivateKey(raw);
  } catch (err) {
    console.error('Failed to restore private key', err);
    return null;
  }
}
