import { createContext } from 'preact';
import { useContext, useState, useEffect, useCallback } from 'preact/hooks';
import { deriveKey } from '../crypto';

interface E2EEState {
  key: CryptoKey | null;
  userId: string | null;
  setKey(password: string, userId: string): Promise<void>;
  clearKey(): void;
}

const E2EEContext = createContext<E2EEState>(null as unknown as E2EEState);

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join('');
}

export function E2EEProvider({ children }: { children: preact.ComponentChildren }) {
  const [key, setKeyState] = useState<CryptoKey | null>(null);
  const [userId, setUserId] = useState<string | null>(null);

  // On mount, restore key from localStorage if available
  useEffect(() => {
    const storedHex = localStorage.getItem('e2ee_key');
    const storedUser = localStorage.getItem('e2ee_user_id');
    if (storedHex && storedUser) {
      crypto.subtle
        .importKey('raw', hexToBytes(storedHex), { name: 'AES-GCM' }, false, ['decrypt'])
        .then((k) => {
          setKeyState(k);
          setUserId(storedUser);
        })
        .catch(() => {
          localStorage.removeItem('e2ee_key');
          localStorage.removeItem('e2ee_user_id');
        });
    }
  }, []);

  const setKey = useCallback(async (password: string, uid: string) => {
    const derived = await deriveKey(password, uid, true);
    const raw = await crypto.subtle.exportKey('raw', derived);
    localStorage.setItem('e2ee_key', bytesToHex(new Uint8Array(raw)));
    localStorage.setItem('e2ee_user_id', uid);
    // Re-import as non-extractable for in-memory use
    const usableKey = await crypto.subtle.importKey('raw', raw, { name: 'AES-GCM' }, false, ['decrypt']);
    setKeyState(usableKey);
    setUserId(uid);
  }, []);

  const clearKey = useCallback(() => {
    localStorage.removeItem('e2ee_key');
    localStorage.removeItem('e2ee_user_id');
    setKeyState(null);
    setUserId(null);
  }, []);

  return (
    <E2EEContext.Provider value={{ key, userId, setKey, clearKey }}>
      {children}
    </E2EEContext.Provider>
  );
}

export function useE2EE() {
  return useContext(E2EEContext);
}
