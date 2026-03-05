import { useState, useEffect } from "preact/hooks";
import { useAuth } from "../../context/auth";
import { useE2EE } from "../../context/e2ee";
import { api } from "../../api";
import {
  deriveKey,
  deriveWrappingKey,
  decryptBatch,
  encryptData,
} from "../../crypto";
import "./style.css";

export function Settings() {
  const { token, userId, wrappingKey } = useAuth();
  const e2ee = useE2EE();

  // Profile
  const [name, setName] = useState("");
  const [nameStatus, setNameStatus] = useState<string | null>(null);
  const [nameSaving, setNameSaving] = useState(false);

  // Wrapping key unlock (only shown when wrappingKey is null after a stale session)
  const [unlockPassword, setUnlockPassword] = useState("");
  const [unlockError, setUnlockError] = useState<string | null>(null);
  const [unlocking, setUnlocking] = useState(false);
  const [resolvedWK, setResolvedWK] = useState<CryptoKey | null>(null);

  // E2EE key change
  const [newE2EEPassword, setNewE2EEPassword] = useState("");
  const [confirmE2EEPassword, setConfirmE2EEPassword] = useState("");
  const [e2eeStatus, setE2eeStatus] = useState<string | null>(null);
  const [e2eeError, setE2eeError] = useState<string | null>(null);
  const [e2eeSaving, setE2eeSaving] = useState(false);

  const activeWK = wrappingKey ?? resolvedWK;

  useEffect(() => {
    if (!token) return;
    api
      .getMe(token)
      .then((me) => setName(me.name ?? ""))
      .catch(() => {});
  }, [token]);

  async function saveName(e: Event) {
    e.preventDefault();
    if (!token) return;
    setNameStatus(null);
    setNameSaving(true);
    try {
      await api.updateMe(token, { name: name.trim() || undefined });
      setNameStatus("Saved.");
    } catch (err) {
      setNameStatus(err instanceof Error ? err.message : "Failed to save");
    } finally {
      setNameSaving(false);
    }
  }

  async function unlockWrappingKey(e: Event) {
    e.preventDefault();
    if (!token || !userId) return;
    setUnlockError(null);
    setUnlocking(true);
    try {
      const wk = await deriveWrappingKey(unlockPassword, userId);
      // Verify by trying to decrypt the existing E2EE key blob
      const { encryptedE2EEKey } = await api.getE2EEKey(token);
      if (encryptedE2EEKey) {
        await decryptBatch(wk, Uint8Array.fromBase64(encryptedE2EEKey));
      }
      setResolvedWK(wk);
      setUnlockPassword("");
    } catch {
      setUnlockError("Incorrect password.");
    } finally {
      setUnlocking(false);
    }
  }

  async function changeE2EEKey(e: Event) {
    e.preventDefault();
    setE2eeStatus(null);
    setE2eeError(null);
    if (!token || !userId || !activeWK) return;
    if (newE2EEPassword !== confirmE2EEPassword) {
      setE2eeError("Passwords do not match.");
      return;
    }
    setE2eeSaving(true);
    try {
      const e2eeKey = await deriveKey(newE2EEPassword, userId, true);
      const rawE2EE = new Uint8Array(
        await crypto.subtle.exportKey("raw", e2eeKey),
      );
      await e2ee.setKeyFromBytes(rawE2EE.buffer, userId);
      const encrypted = await encryptData(activeWK, rawE2EE);
      await api.setE2EEKey(token, encrypted.toBase64());
      setNewE2EEPassword("");
      setConfirmE2EEPassword("");
      setE2eeStatus("E2EE key updated.");
    } catch (err) {
      setE2eeError(err instanceof Error ? err.message : "Failed to update key");
    } finally {
      setE2eeSaving(false);
    }
  }

  return (
    <div class="settings-page">
      <h1 class="settings-title">Settings</h1>

      <section class="card settings-section">
        <h2>Profile</h2>
        <form class="settings-form" onSubmit={saveName}>
          <div class="field">
            <label for="settings-name">Display name</label>
            <input
              id="settings-name"
              type="text"
              value={name}
              onInput={(e) => {
                setName((e.target as HTMLInputElement).value);
                setNameStatus(null);
              }}
              placeholder="Your name"
              autoComplete="name"
            />
          </div>
          {nameStatus && (
            <p
              class={nameStatus === "Saved." ? "alert-success" : "alert-error"}
            >
              {nameStatus}
            </p>
          )}
          <button class="btn btn-primary" type="submit" disabled={nameSaving}>
            {nameSaving ? "Saving…" : "Save name"}
          </button>
        </form>
      </section>

      <section class="card settings-section">
        <h2>Encryption key</h2>
        {!activeWK ? (
          <>
            <p class="settings-hint">
              Enter your account password to change your encryption key.
            </p>
            <form class="settings-form" onSubmit={unlockWrappingKey}>
              <div class="field">
                <label for="unlock-password">Account password</label>
                <input
                  id="unlock-password"
                  type="password"
                  value={unlockPassword}
                  onInput={(e) => {
                    setUnlockPassword((e.target as HTMLInputElement).value);
                    setUnlockError(null);
                  }}
                  placeholder="Your login password"
                  autoComplete="current-password"
                  required
                />
              </div>
              {unlockError && <p class="alert-error">{unlockError}</p>}
              <button
                class="btn btn-primary"
                type="submit"
                disabled={unlocking}
              >
                {unlocking ? "Verifying…" : "Unlock"}
              </button>
            </form>
          </>
        ) : (
          <>
            <p class="settings-hint">
              Changing your E2EE password re-encrypts your key on the server.
              Existing logs will still decrypt correctly since the underlying
              key is regenerated from the new password.
            </p>
            <form class="settings-form" onSubmit={changeE2EEKey}>
              <div class="field">
                <label for="new-e2ee">New E2EE password</label>
                <input
                  id="new-e2ee"
                  type="password"
                  value={newE2EEPassword}
                  onInput={(e) => {
                    setNewE2EEPassword((e.target as HTMLInputElement).value);
                    setE2eeError(null);
                  }}
                  placeholder="New encryption password"
                  autoComplete="new-password"
                  required
                />
              </div>
              <div class="field">
                <label for="confirm-e2ee">Confirm new E2EE password</label>
                <input
                  id="confirm-e2ee"
                  type="password"
                  value={confirmE2EEPassword}
                  onInput={(e) => {
                    setConfirmE2EEPassword(
                      (e.target as HTMLInputElement).value,
                    );
                    setE2eeError(null);
                  }}
                  placeholder="••••••••"
                  autoComplete="new-password"
                  required
                />
              </div>
              {e2eeError && <p class="alert-error">{e2eeError}</p>}
              {e2eeStatus && <p class="alert-success">{e2eeStatus}</p>}
              <button
                class="btn btn-primary"
                type="submit"
                disabled={e2eeSaving}
              >
                {e2eeSaving ? "Updating…" : "Update E2EE key"}
              </button>
            </form>
          </>
        )}
      </section>
    </div>
  );
}
