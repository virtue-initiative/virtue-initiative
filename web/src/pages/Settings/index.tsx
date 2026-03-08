import { useState, useEffect } from "preact/hooks";
import { useAuth } from "../../context/auth";
import { api } from "../../api";
import "./style.css";

export function Settings() {
  const { token } = useAuth();

  const [name, setName] = useState("");
  const [nameStatus, setNameStatus] = useState<string | null>(null);
  const [nameSaving, setNameSaving] = useState(false);

  useEffect(() => {
    if (!token) return;
    api
      .getUser(token)
      .then((user) => setName(user.name ?? ""))
      .catch(() => {});
  }, [token]);

  async function saveName(e: Event) {
    e.preventDefault();
    if (!token) return;
    setNameStatus(null);
    setNameSaving(true);
    try {
      await api.updateUser(token, { name: name.trim() || undefined });
      setNameStatus("Saved.");
    } catch (err) {
      setNameStatus(err instanceof Error ? err.message : "Failed to save");
    } finally {
      setNameSaving(false);
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
        <p class="settings-hint">
          Encryption keys are generated and restored automatically. Older
          accounts will save their partner-sharing keypair the next time they
          sign in.
        </p>
      </section>
    </div>
  );
}
