import { useEffect, useState } from "preact/hooks";
import { api, User, WatchingPartner } from "../../api";
import { useAuth } from "../../context/auth";
import "./style.css";

export function Settings() {
  const { token } = useAuth();

  const [user, setUser] = useState<User | null>(null);
  const [watching, setWatching] = useState<WatchingPartner[]>([]);
  const [email, setEmail] = useState("");
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
    const [nextUser, nextPartners] = await Promise.all([
      api.getUser(token),
      api.getPartners(token),
    ]);
    setUser(nextUser);
    setEmail(nextUser.email);
    setName(nextUser.name ?? "");
    setWatching(nextPartners.watching);
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
      const nextEmail = email.trim().toLowerCase();
      const emailChanged = user ? nextEmail !== user.email : false;
      await api.updateUser(token, {
        email: emailChanged ? nextEmail : undefined,
        name: name.trim() || undefined,
      });
      setNameStatus(
        emailChanged
          ? "Profile saved. Please verify your new email address."
          : "Saved.",
      );
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
      Pick<WatchingPartner, "digest_cadence" | "immediate_tamper_severity">
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
          <div class="field">
            <label for="settings-email">Email</label>
            <input
              id="settings-email"
              type="email"
              value={email}
              onInput={(e) => {
                setEmail((e.target as HTMLInputElement).value);
                setNameStatus(null);
              }}
              placeholder="you@example.com"
              autoComplete="email"
              required
            />
          </div>
          {nameStatus && (
            <p
              class={
                nameStatus.toLowerCase().includes("saved")
                  ? "alert-success"
                  : "alert-error"
              }
            >
              {nameStatus}
            </p>
          )}
          <button class="btn btn-primary" type="submit" disabled={nameSaving}>
            {nameSaving ? "Saving…" : "Save"}
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
            {Boolean(user?.email_bounced_at) && (
              <p class="alert-error">
                Your last verification email bounced. Update your email above
                before requesting another verification email.
              </p>
            )}
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
              disabled={verificationSending || Boolean(user?.email_bounced_at)}
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

        {watching.length === 0 ? (
          <p class="settings-hint">
            Accept a partner invite to configure email notifications for the
            people you monitor.
          </p>
        ) : (
          <div class="settings-list">
            {watching.map((partner) => (
              <div class="settings-item" key={partner.id}>
                <div class="settings-item-header">
                  <strong>
                    Monitoring {partner.user.name ?? partner.user.email}
                  </strong>
                  <span class="settings-badge">{partner.status}</span>
                </div>
                {partner.status === "accepted" && (
                  <div class="settings-preference-grid">
                    <label class="field settings-inline-field">
                      <span>Email notifications</span>
                      <select
                        class="settings-select"
                        value={partner.digest_cadence}
                        onChange={(e) =>
                          updatePreference(partner.id, {
                            digest_cadence: (e.target as HTMLSelectElement)
                              .value as WatchingPartner["digest_cadence"],
                          }).catch(() => {})
                        }
                        disabled={savingPreferenceId === partner.id}
                      >
                        <option value="none">None</option>
                        <option value="alerts-only">Alerts only</option>
                        <option value="daily">Daily</option>
                        <option value="weekly">Weekly</option>
                      </select>
                    </label>
                    <label class="field settings-inline-field">
                      <span>Immediate tamper emails</span>
                      <select
                        class="settings-select"
                        value={partner.immediate_tamper_severity}
                        onChange={(e) =>
                          updatePreference(partner.id, {
                            immediate_tamper_severity: (
                              e.target as HTMLSelectElement
                            )
                              .value as WatchingPartner["immediate_tamper_severity"],
                          }).catch(() => {})
                        }
                        disabled={
                          savingPreferenceId === partner.id ||
                          partner.digest_cadence === "none"
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
