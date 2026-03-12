import { useEffect, useState } from "preact/hooks";
import { api, NotificationPreference, User } from "../../api";
import { useAuth } from "../../context/auth";
import "./style.css";

export function Settings() {
  const { token } = useAuth();

  const [user, setUser] = useState<User | null>(null);
  const [preferences, setPreferences] = useState<NotificationPreference[]>([]);
  const [name, setName] = useState("");
  const [nameStatus, setNameStatus] = useState<string | null>(null);
  const [verificationStatus, setVerificationStatus] = useState<string | null>(
    null,
  );
  const [nameSaving, setNameSaving] = useState(false);
  const [verificationSending, setVerificationSending] = useState(false);
  const [savingPreferenceId, setSavingPreferenceId] = useState<string | null>(
    null,
  );

  async function reload() {
    if (!token) return;
    const [nextUser, nextPreferences] = await Promise.all([
      api.getUser(token),
      api.getNotificationPreferences(token),
    ]);
    setUser(nextUser);
    setName(nextUser.name ?? "");
    setPreferences(nextPreferences);
  }

  useEffect(() => {
    reload().catch(() => {});
  }, [token]);

  async function saveName(e: Event) {
    e.preventDefault();
    if (!token) return;
    setNameStatus(null);
    setNameSaving(true);
    try {
      await api.updateUser(token, { name: name.trim() || undefined });
      setNameStatus("Saved.");
      await reload();
    } catch (err) {
      setNameStatus(err instanceof Error ? err.message : "Failed to save");
    } finally {
      setNameSaving(false);
    }
  }

  async function resendVerificationEmail() {
    if (!token) return;
    setVerificationStatus(null);
    setVerificationSending(true);
    try {
      const result = await api.requestVerificationEmail(token);
      setVerificationStatus(
        result.already_verified
          ? "Your email is already verified."
          : "Verification email sent.",
      );
      await reload();
    } catch (err) {
      setVerificationStatus(
        err instanceof Error ? err.message : "Failed to send email",
      );
    } finally {
      setVerificationSending(false);
    }
  }

  async function updatePreference(
    partnershipId: string,
    patch: Partial<
      Pick<
        NotificationPreference,
        "digest_cadence" | "immediate_tamper_severity" | "send_digest"
      >
    >,
  ) {
    if (!token) return;
    setSavingPreferenceId(partnershipId);
    try {
      await api.updateNotificationPreference(token, partnershipId, patch);
      await reload();
    } finally {
      setSavingPreferenceId(null);
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
        <h2>Email verification</h2>
        <p class="settings-hint">
          {user?.email_verified
            ? `Your email (${user.email}) is verified.`
            : `Your email (${user?.email ?? "loading…"}) is not verified yet.`}
        </p>
        {!user?.email_verified && (
          <>
            {verificationStatus && (
              <p
                class={
                  verificationStatus.includes("sent") ||
                  verificationStatus.includes("already")
                    ? "alert-success"
                    : "alert-error"
                }
              >
                {verificationStatus}
              </p>
            )}
            <button
              class="btn btn-primary"
              type="button"
              disabled={verificationSending}
              onClick={resendVerificationEmail}
            >
              {verificationSending ? "Sending…" : "Resend verification email"}
            </button>
          </>
        )}
      </section>

      <section class="card settings-section">
        <h2>Partner notifications</h2>
        <p class="settings-hint">
          Configure how you receive tamper alerts and summary emails for each
          person you monitor.
        </p>

        {preferences.length === 0 ? (
          <p class="settings-hint">
            Accept a partner invite to configure email notifications for the
            people you monitor.
          </p>
        ) : (
          <div class="settings-list">
            {preferences.map((preference) => (
              <div class="settings-item" key={preference.partnership_id}>
                <div class="settings-item-header">
                  <strong>
                    Monitoring{" "}
                    {preference.monitored_user.name ??
                      preference.monitored_user.email}
                  </strong>
                  <span class="settings-badge">{preference.status}</span>
                </div>
                <label class="settings-checkbox">
                  <input
                    type="checkbox"
                    checked={preference.send_digest}
                    onChange={(e) =>
                      updatePreference(preference.partnership_id, {
                        send_digest: (e.target as HTMLInputElement).checked,
                      }).catch(() => {})
                    }
                    disabled={savingPreferenceId === preference.partnership_id}
                  />
                  <span>Receive emails</span>
                </label>
                {preference.send_digest && (
                  <div class="settings-preference-grid">
                    <label class="field settings-inline-field">
                      <span>Digest cadence</span>
                      <select
                        class="settings-select"
                        value={preference.digest_cadence}
                        onChange={(e) =>
                          updatePreference(preference.partnership_id, {
                            digest_cadence: (e.target as HTMLSelectElement)
                              .value as NotificationPreference["digest_cadence"],
                          }).catch(() => {})
                        }
                        disabled={
                          savingPreferenceId === preference.partnership_id
                        }
                      >
                        <option value="none">None</option>
                        <option value="daily">Daily</option>
                        <option value="twice_weekly">
                          Twice a week (Sunday and Wednesday)
                        </option>
                        <option value="weekly">Weekly</option>
                      </select>
                    </label>
                    <label class="field settings-inline-field">
                      <span>Immediate tamper emails</span>
                      <select
                        class="settings-select"
                        value={preference.immediate_tamper_severity}
                        onChange={(e) =>
                          updatePreference(preference.partnership_id, {
                            immediate_tamper_severity: (
                              e.target as HTMLSelectElement
                            )
                              .value as NotificationPreference["immediate_tamper_severity"],
                          }).catch(() => {})
                        }
                        disabled={
                          savingPreferenceId === preference.partnership_id
                        }
                      >
                        <option value="critical">Critical only</option>
                        <option value="warning">Warning and critical</option>
                      </select>
                    </label>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
