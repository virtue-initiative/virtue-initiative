import { createContext } from 'preact';
import { useContext, useState, useEffect, useCallback } from 'preact/hooks';
import { api, setReauthHandler } from '../api';
import { clearDataCache } from '../data-cache';
import { DEFAULT_DIGEST_LOCAL_HOUR, localHourToUtcMinutes } from '../utils/digest';
import {
  derivePasswordMaterial,
  encryptData,
  generateRandomKeyBytes,
  generateUserKeyPair,
} from '../crypto';

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

interface AuthState {
  token: string | null;
  userId: string | null;
  wrappingKey: CryptoKey | null;
  ready: boolean;
  login: (
    email: string,
    password: string,
  ) => Promise<{
    access_token: string;
    userId: string;
    wrappingKey: CryptoKey;
  }>;
  signup: (
    email: string,
    password: string,
    name?: string,
    partnerInviteToken?: string,
  ) => Promise<{
    userId: string;
    wrappingKey: CryptoKey;
    email: string;
  }>;
  verifyEmail: (token: string) => Promise<{
    access_token: string;
    userId: string;
    email: string;
    purpose: 'email_verification' | 'email_change';
  }>;
  rememberWrappingKey: (wrappingKey: CryptoKey) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthState>(null as unknown as AuthState);

export function AuthProvider({ children }: { children: preact.ComponentChildren }) {
  const [token, setToken] = useState<string | null>(null);
  const [userId, setUserId] = useState<string | null>(null);
  const [wrappingKey, setWrappingKey] = useState<CryptoKey | null>(null);
  const [ready, setReady] = useState(false);

  function jwtSub(t: string): string | null {
    try {
      const b64 = t.split('.')[1].replace(/-/g, '+').replace(/_/g, '/');
      const padded = b64 + '='.repeat((4 - (b64.length % 4)) % 4);
      return JSON.parse(atob(padded)).sub ?? null;
    } catch {
      return null;
    }
  }

  useEffect(() => {
    Promise.all([
      api
        .refreshToken()
        .then((res) => {
          setToken(res.access_token);
          setUserId(jwtSub(res.access_token));
        })
        .catch(() => {}),
      loadWrappingKey()
        .then((wk) => {
          if (wk) setWrappingKey(wk);
        })
        .catch(() => {}),
    ]).finally(() => setReady(true));
  }, []);

  const login = useCallback(async (email: string, pw: string) => {
    const material = await api.getLoginMaterial(email);
    const { passwordAuth, wrappingKey: wk } = await derivePasswordMaterial(
      pw,
      Uint8Array.fromBase64(material.password_salt),
      material.params,
    );
    const res = await api.login(email, passwordAuth.toBase64());
    const uid = jwtSub(res.access_token)!;
    await saveWrappingKey(wk);
    setToken(res.access_token);
    setUserId(uid);
    setWrappingKey(wk);
    return { access_token: res.access_token, userId: uid, wrappingKey: wk };
  }, []);

  const signup = useCallback(
    async (email: string, pw: string, name?: string, partnerInviteToken?: string) => {
      const params = await api.getCurrentHashParams();
      const passwordSalt = generateRandomKeyBytes(params.salt_length);
      const { passwordAuth, wrappingKey: wk } = await derivePasswordMaterial(
        pw,
        passwordSalt,
        params,
      );
      const keyPair = await generateUserKeyPair();
      const encryptedPrivateKey = await encryptData(wk, keyPair.privateKey);
      const res = await api.signup(email, {
        password_auth: passwordAuth.toBase64(),
        password_salt: passwordSalt.toBase64(),
        pub_key: keyPair.publicKey.toBase64(),
        priv_key: encryptedPrivateKey.toBase64(),
        ...(name ? { name } : {}),
        email_digest_minutes_utc: localHourToUtcMinutes(DEFAULT_DIGEST_LOCAL_HOUR),
        ...(partnerInviteToken ? { partner_invite_token: partnerInviteToken } : {}),
      });
      await saveWrappingKey(wk);
      setWrappingKey(wk);
      return {
        userId: res.user.id,
        wrappingKey: wk,
        email: res.user.email,
      };
    },
    [],
  );

  const verifyEmail = useCallback(async (token: string) => {
    const res = await api.verifyEmail(token);
    const uid = jwtSub(res.access_token);

    if (!uid) {
      throw new Error('Verified access token is missing a subject');
    }

    const persistedWrappingKey = await loadWrappingKey().catch(() => null);
    if (persistedWrappingKey) {
      setWrappingKey(persistedWrappingKey);
    }

    setToken(res.access_token);
    setUserId(uid);
    return {
      access_token: res.access_token,
      userId: uid,
      email: res.email,
      purpose: res.purpose,
    };
  }, []);

  const refresh = useCallback(async () => {
    const res = await api.refreshToken();
    const uid = jwtSub(res.access_token);

    if (!uid) {
      throw new Error('Refreshed access token is missing a subject');
    }

    setToken(res.access_token);
    setUserId(uid);
    return res.access_token;
  }, []);

  const logout = useCallback(async () => {
    await api.logout().catch(() => {});
    await clearDataCache().catch(() => {});
    clearWrappingKey();
    setToken(null);
    setUserId(null);
    setWrappingKey(null);
  }, []);

  const rememberWrappingKey = useCallback(async (nextWrappingKey: CryptoKey) => {
    await saveWrappingKey(nextWrappingKey);
    setWrappingKey(nextWrappingKey);
  }, []);

  useEffect(() => {
    setReauthHandler(async () => {
      try {
        return await refresh();
      } catch {
        void clearDataCache().catch(() => {});
        clearWrappingKey();
        setToken(null);
        setUserId(null);
        setWrappingKey(null);
        return null;
      }
    });

    return () => setReauthHandler(null);
  }, [refresh]);

  return (
    <AuthContext.Provider
      value={{
        token,
        userId,
        wrappingKey,
        ready,
        login,
        signup,
        verifyEmail,
        rememberWrappingKey,
        logout,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
