import { createContext } from 'preact';
import { useContext, useState, useEffect, useCallback } from 'preact/hooks';
import { api } from '../api';
import { deriveWrappingKey, hashPasswordForAuth } from '../crypto';

interface AuthState {
  token: string | null;
  userId: string | null;
  wrappingKey: CryptoKey | null;
  ready: boolean;
  login: (email: string, password: string) => Promise<{ access_token: string; userId: string; wrappingKey: CryptoKey }>;
  signup: (email: string, password: string, name?: string) => Promise<{ access_token: string; userId: string; wrappingKey: CryptoKey }>;
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

  // On mount, try to restore session from the httpOnly refresh cookie
  useEffect(() => {
    api.refreshToken()
      .then((res) => {
        setToken(res.access_token);
        setUserId(jwtSub(res.access_token));
      })
      .catch(() => {})
      .finally(() => setReady(true));
  }, []);

  const login = useCallback(async (email: string, pw: string) => {
    const pwHash = await hashPasswordForAuth(pw, email);
    const res = await api.login(email, pwHash);
    const uid = jwtSub(res.access_token)!;
    const wk = await deriveWrappingKey(pw, uid);
    setToken(res.access_token);
    setUserId(uid);
    setWrappingKey(wk);
    return { access_token: res.access_token, userId: uid, wrappingKey: wk };
  }, []);

  const signup = useCallback(async (email: string, pw: string, name?: string) => {
    const pwHash = await hashPasswordForAuth(pw, email);
    const res = await api.signup(email, pwHash, name);
    const uid = (res.user as { id: string }).id;
    const wk = await deriveWrappingKey(pw, uid);
    setToken(res.access_token);
    setUserId(uid);
    setWrappingKey(wk);
    return { access_token: res.access_token, userId: uid, wrappingKey: wk };
  }, []);

  const logout = useCallback(async () => {
    if (token) await api.logout(token).catch(() => {});
    setToken(null);
    setUserId(null);
    setWrappingKey(null);
  }, [token]);

  return (
    <AuthContext.Provider value={{ token, userId, wrappingKey, ready, login, signup, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
