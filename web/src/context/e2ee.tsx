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
import {
  E2EEKeyRange,
  decodeE2EEKeyRanges,
  latestE2EEKeyRange,
  normalizeE2EEKeyRanges,
  selectE2EEKeyRangeForTimestamp,
} from "../lib/e2ee-keyring";
import { useAuth } from "./auth";

interface E2EEState {
  keys: Record<string, CryptoKey>;
  getKey(userId: string): CryptoKey | null;
  getKeyBytes(userId: string): Uint8Array | null;
  getKeyBytesForTimestamp(userId: string, timestamp: number): Uint8Array | null;
  getKeyRanges(userId: string): E2EEKeyRange[];
  setKey(password: string, userId: string): Promise<void>;
  setKeyFromBytes(rawBytes: ArrayBuffer, userId: string): Promise<void>;
  setKeyRanges(ranges: E2EEKeyRange[], userId: string): Promise<void>;
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
  const [keyRangesByUser, setKeyRangesByUser] = useState<
    Record<string, E2EEKeyRange[]>
  >({});

  const getKey = useCallback(
    (userId: string): CryptoKey | null => keys[userId] ?? null,
    [keys],
  );

  const getKeyBytes = useCallback(
    (uid: string) => {
      const latestRange = latestE2EEKeyRange(keyRangesByUser[uid] ?? []);
      return latestRange?.key ?? getStoredKeyBytes(uid);
    },
    [keyRangesByUser],
  );

  const getKeyBytesForTimestamp = useCallback(
    (uid: string, timestamp: number) => {
      const ranges = keyRangesByUser[uid] ?? [];
      if (ranges.length === 0) {
        return getStoredKeyBytes(uid);
      }
      return selectE2EEKeyRangeForTimestamp(ranges, timestamp)?.key ?? null;
    },
    [keyRangesByUser],
  );

  const getKeyRanges = useCallback(
    (uid: string) => keyRangesByUser[uid] ?? [],
    [keyRangesByUser],
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

  const setKeyRanges = useCallback(
    async (ranges: E2EEKeyRange[], uid: string) => {
      const normalized = normalizeE2EEKeyRanges(ranges);
      const latestRange = latestE2EEKeyRange(normalized);
      if (!latestRange) {
        return;
      }

      localStorage.setItem(LS_PREFIX + uid, bytesToHex(latestRange.key));
      const keyMaterial = Uint8Array.from(latestRange.key);
      const usableKey = await crypto.subtle.importKey(
        "raw",
        keyMaterial,
        { name: "AES-GCM" },
        false,
        ["decrypt"],
      );

      setKeyRangesByUser((prev) => ({ ...prev, [uid]: normalized }));
      setKeys((prev) => ({ ...prev, [uid]: usableKey }));
    },
    [],
  );

  const setKeyFromBytes = useCallback(
    async (rawBytes: ArrayBuffer, uid: string) => {
      await setKeyRanges(
        [
          {
            start: 0,
            end: null,
            key: Uint8Array.from(new Uint8Array(rawBytes)),
          },
        ],
        uid,
      );
    },
    [setKeyRanges],
  );

  const clearKey = useCallback((userId?: string) => {
    if (userId) {
      localStorage.removeItem(LS_PREFIX + userId);
      setKeys((prev) => {
        const next = { ...prev };
        delete next[userId];
        return next;
      });
      setKeyRangesByUser((prev) => {
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
      setKeyRangesByUser({});
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
          const decryptedE2EE = await decryptBatch(
            wrappingKey,
            Uint8Array.fromBase64(user.e2ee_key),
          );
          const ranges = decodeE2EEKeyRanges(decryptedE2EE);
          if (!cancelled) {
            await setKeyRanges(ranges, userId);
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
          partners
            .filter(
              (partner) =>
                partner.role === "invitee" &&
                partner.status === "accepted" &&
                partner.permissions.view_data &&
                partner.e2ee_key &&
                partner.partner.id,
            )
            .map(async (partner) => {
              const rawKey = await decryptWithPrivateKey(
                privateKey,
                Uint8Array.fromBase64(partner.e2ee_key!),
              );
              if (!cancelled) {
                await setKeyFromBytes(rawKey.buffer, partner.partner.id!);
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
  }, [clearKey, token, userId, wrappingKey, setKeyFromBytes, setKeyRanges]);

  return (
    <E2EEContext.Provider
      value={{
        keys,
        getKey,
        getKeyBytes,
        getKeyBytesForTimestamp,
        getKeyRanges,
        setKey,
        setKeyFromBytes,
        setKeyRanges,
        clearKey,
      }}
    >
      {children}
    </E2EEContext.Provider>
  );
}

export function useE2EE() {
  return useContext(E2EEContext);
}
