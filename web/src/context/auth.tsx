import { createContext } from 'preact';
import { useContext, useState, useEffect, useCallback } from 'preact/hooks';
import { api } from '../api';

interface AuthState {
  token: string | null;
  ready: boolean;
  login: (email: string, password: string) => Promise<void>;
  signup: (email: string, password: string, name?: string) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthState>(null as unknown as AuthState);

export function AuthProvider({ children }: { children: preact.ComponentChildren }) {
  const [token, setToken] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  // On mount, try to restore session from the httpOnly refresh cookie
  useEffect(() => {
    api.refreshToken()
      .then((res) => setToken(res.access_token))
      .catch(() => {})
      .finally(() => setReady(true));
  }, []);

  const login = useCallback(async (email: string, password: string) => {
    const res = await api.login(email, password);
    setToken(res.access_token);
  }, []);

  const signup = useCallback(async (email: string, password: string, name?: string) => {
    const res = await api.signup(email, password, name);
    setToken(res.access_token);
  }, []);

  const logout = useCallback(async () => {
    if (token) await api.logout(token).catch(() => {});
    setToken(null);
  }, [token]);

  return (
    <AuthContext.Provider value={{ token, ready, login, signup, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  return useContext(AuthContext);
}
