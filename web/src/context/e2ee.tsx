import { createContext } from "preact";
import { useContext, useState, useEffect, useCallback } from "preact/hooks";
import { api } from "../api";
import {
  decryptBatch,
  decryptWithPrivateKey,
  deriveKey,
  encryptData,
  exportPrivateKey,
  exportPublicKey,
  generateSharingKeyPair,
  importPrivateKey,
} from "../crypto";
import { useAuth } from "./auth";

interface E2EEState {
  keys: Record<string, CryptoKey>;
  getKey(userId: string): CryptoKey | null;
  getKeyBytes(userId: string): Uint8Array | null;
  setKey(password: string, userId: string): Promise<void>;
  setKeyFromBytes(rawBytes: ArrayBuffer, userId: string): Promise<void>;
  clearKey(userId?: string): void;
}

const E2EEContext = createContext<E2EEState>(null as unknown as E2EEState);

const LS_PREFIX = "e2ee_key_";

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function getStoredKeyBytes(userId: string): Uint8Array<ArrayBuffer> | null {
  const hex = localStorage.getItem(LS_PREFIX + userId);
  return hex ? Uint8Array.from(hexToBytes(hex)) : null;
}

export function E2EEProvider({
  children,
}: {
  children: preact.ComponentChildren;
}) {
  const { token, userId, wrappingKey } = useAuth();
  const [keys, setKeys] = useState<Record<string, CryptoKey>>({});

  const getKey = useCallback(
    (userId: string): CryptoKey | null => keys[userId] ?? null,
    [keys],
  );

  const getKeyBytes = useCallback(
    (userId: string) => getStoredKeyBytes(userId),
    [],
  );

  const setKey = useCallback(async (password: string, uid: string) => {
    const derived = await deriveKey(password, uid, true);
    const raw = await crypto.subtle.exportKey("raw", derived);
    localStorage.setItem(LS_PREFIX + uid, bytesToHex(new Uint8Array(raw)));
    const usableKey = await crypto.subtle.importKey(
      "raw",
      raw,
      { name: "AES-GCM" },
      false,
      ["decrypt"],
    );
    setKeys((prev) => ({ ...prev, [uid]: usableKey }));
  }, []);

  const setKeyFromBytes = useCallback(
    async (rawBytes: ArrayBuffer, uid: string) => {
      localStorage.setItem(
        LS_PREFIX + uid,
        bytesToHex(new Uint8Array(rawBytes)),
      );
      const usableKey = await crypto.subtle.importKey(
        "raw",
        rawBytes,
        { name: "AES-GCM" },
        false,
        ["decrypt"],
      );
      setKeys((prev) => ({ ...prev, [uid]: usableKey }));
    },
    [],
  );

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

  useEffect(() => {
    if (!token || !userId || !wrappingKey) {
      clearKey();
      return;
    }

    let cancelled = false;

    async function restoreKeyMaterial() {
      try {
        clearKey();

        const user = await api.getUser(token);
        if (cancelled) return;

        if (user.e2ee_key) {
          const rawE2EE = await decryptBatch(
            wrappingKey,
            Uint8Array.fromBase64(user.e2ee_key),
          );
          if (!cancelled) {
            await setKeyFromBytes(rawE2EE.buffer, userId);
          }
        }

        let privateKeyBytes: Uint8Array<ArrayBuffer> | null = null;
        if (user.priv_key) {
          privateKeyBytes = await decryptBatch(
            wrappingKey,
            Uint8Array.fromBase64(user.priv_key),
          );
        }

        if (!user.pub_key || !user.priv_key) {
          const keyPair = await generateSharingKeyPair();
          const publicKeyBytes = await exportPublicKey(keyPair.publicKey);
          privateKeyBytes = await exportPrivateKey(keyPair.privateKey);
          const encryptedPrivateKey = await encryptData(
            wrappingKey,
            privateKeyBytes,
          );
          await api.updateUser(token, {
            pub_key: publicKeyBytes.toBase64(),
            priv_key: encryptedPrivateKey.toBase64(),
          });
        }

        if (!privateKeyBytes || cancelled) return;

        const privateKey = await importPrivateKey(privateKeyBytes);
        const partners = await api.getPartners(token);

        await Promise.all(
          partners.watching
            .filter(
              (partner) =>
                partner.status === "accepted" &&
                partner.e2ee_key &&
                partner.user.id,
            )
            .map(async (partner) => {
              const rawKey = await decryptWithPrivateKey(
                privateKey,
                Uint8Array.fromBase64(partner.e2ee_key!),
              );
              if (!cancelled) {
                await setKeyFromBytes(rawKey.buffer, partner.user.id);
              }
            }),
        );
      } catch (err) {
        console.error("Failed to restore E2EE key material", err);
      }
    }

    void restoreKeyMaterial();

    return () => {
      cancelled = true;
    };
  }, [clearKey, token, userId, wrappingKey, setKeyFromBytes]);

  return (
    <E2EEContext.Provider
      value={{ keys, getKey, getKeyBytes, setKey, setKeyFromBytes, clearKey }}
    >
      {children}
    </E2EEContext.Provider>
  );
}

export function useE2EE() {
  return useContext(E2EEContext);
}
