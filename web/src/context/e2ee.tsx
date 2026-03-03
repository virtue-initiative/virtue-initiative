import { createContext } from 'preact';
import { useContext, useState, useEffect, useCallback } from 'preact/hooks';
import { deriveKey } from '../crypto';

interface E2EEState {
  keys: Record<string, CryptoKey>;
  getKey(userId: string): CryptoKey | null;
  setKey(password: string, userId: string): Promise<void>;
  setKeyFromBytes(rawBytes: ArrayBuffer, userId: string): Promise<void>;
  clearKey(userId?: string): void;
}

const E2EEContext = createContext<E2EEState>(null as unknown as E2EEState);

const LS_PREFIX = 'e2ee_key_';

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
  const [keys, setKeys] = useState<Record<string, CryptoKey>>({});

  // On mount, restore all per-user keys from localStorage
  useEffect(() => {
    const entries: Array<[string, string]> = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k?.startsWith(LS_PREFIX)) {
        const uid = k.slice(LS_PREFIX.length);
        const hex = localStorage.getItem(k)!;
        entries.push([uid, hex]);
      }
    }
    if (entries.length === 0) return;
    Promise.all(
      entries.map(([uid, hex]) =>
        crypto.subtle
          .importKey('raw', Uint8Array.from(hexToBytes(hex)), { name: 'AES-GCM' }, false, ['decrypt'])
          .then((ck) => [uid, ck] as [string, CryptoKey])
          .catch(() => {
            localStorage.removeItem(LS_PREFIX + uid);
            return null;
          }),
      ),
    ).then((results) => {
      const loaded: Record<string, CryptoKey> = {};
      for (const r of results) {
        if (r) loaded[r[0]] = r[1];
      }
      setKeys(loaded);
    });
  }, []);

  const getKey = useCallback((userId: string): CryptoKey | null => keys[userId] ?? null, [keys]);

  const setKey = useCallback(async (password: string, uid: string) => {
    const derived = await deriveKey(password, uid, true);
    const raw = await crypto.subtle.exportKey('raw', derived);
    localStorage.setItem(LS_PREFIX + uid, bytesToHex(new Uint8Array(raw)));
    const usableKey = await crypto.subtle.importKey('raw', raw, { name: 'AES-GCM' }, false, ['decrypt']);
    setKeys((prev) => ({ ...prev, [uid]: usableKey }));
  }, []);

  const setKeyFromBytes = useCallback(async (rawBytes: ArrayBuffer, uid: string) => {
    localStorage.setItem(LS_PREFIX + uid, bytesToHex(new Uint8Array(rawBytes)));
    const usableKey = await crypto.subtle.importKey('raw', rawBytes, { name: 'AES-GCM' }, false, ['decrypt']);
    setKeys((prev) => ({ ...prev, [uid]: usableKey }));
  }, []);

  const clearKey = useCallback((userId?: string) => {
    if (userId) {
      localStorage.removeItem(LS_PREFIX + userId);
      setKeys((prev) => {
        const next = { ...prev };
        delete next[userId];
        return next;
      });
    } else {
      for (let i = localStorage.length - 1; i >= 0; i--) {
        const k = localStorage.key(i);
        if (k?.startsWith(LS_PREFIX)) localStorage.removeItem(k);
      }
      setKeys({});
    }
  }, []);

  return (
    <E2EEContext.Provider value={{ keys, getKey, setKey, setKeyFromBytes, clearKey }}>
      {children}
    </E2EEContext.Provider>
  );
}

export function useE2EE() {
  return useContext(E2EEContext);
}
