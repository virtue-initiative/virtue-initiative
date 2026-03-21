import { createContext } from "preact";
import { useContext, useEffect, useState, useCallback } from "preact/hooks";
import { api } from "../api";
import { decryptBatch, importUserPrivateKey, unwrapBatchKey } from "../crypto";
import { useAuth } from "./auth";

interface E2EEState {
  privateKey: CryptoKey | null;
  ready: boolean;
  setPrivateKey: (key: CryptoKey | null) => void;
  clearKey: () => void;
  unwrapEncryptedBatchKey: (encryptedKey: string) => Promise<CryptoKey>;
}

const E2EEContext = createContext<E2EEState>(null as unknown as E2EEState);

export function E2EEProvider({
  children,
}: {
  children: preact.ComponentChildren;
}) {
  const { token, wrappingKey } = useAuth();
  const [privateKey, setPrivateKeyState] = useState<CryptoKey | null>(null);
  const [ready, setReady] = useState(false);

  const setPrivateKey = useCallback((key: CryptoKey | null) => {
    setPrivateKeyState(key);
  }, []);

  const clearKey = useCallback(() => {
    setPrivateKeyState(null);
  }, []);

  const unwrapEncryptedBatchKey = useCallback(
    async (encryptedKey: string) => {
      if (!privateKey) {
        throw new Error("Private key is not available");
      }

      return unwrapBatchKey(privateKey, Uint8Array.fromBase64(encryptedKey));
    },
    [privateKey],
  );

  useEffect(() => {
    if (!token || !wrappingKey) {
      clearKey();
      setReady(true);
      return;
    }

    let cancelled = false;
    setReady(false);

    async function restorePrivateKey() {
      try {
        const user = await api.getUser(token);
        if (cancelled) return;

        if (!user.priv_key) {
          setPrivateKeyState(null);
          return;
        }

        const rawPrivateKey = await decryptBatch(
          wrappingKey,
          Uint8Array.fromBase64(user.priv_key),
        );
        if (cancelled) return;

        setPrivateKeyState(await importUserPrivateKey(rawPrivateKey));
      } catch (error) {
        console.error("Failed to restore private key", error);
        if (!cancelled) {
          setPrivateKeyState(null);
        }
      } finally {
        if (!cancelled) {
          setReady(true);
        }
      }
    }

    void restorePrivateKey();

    return () => {
      cancelled = true;
    };
  }, [clearKey, token, wrappingKey]);

  return (
    <E2EEContext.Provider
      value={{
        privateKey,
        ready,
        setPrivateKey,
        clearKey,
        unwrapEncryptedBatchKey,
      }}
    >
      {children}
    </E2EEContext.Provider>
  );
}

export function useE2EE() {
  return useContext(E2EEContext);
}
