import { useEffect, useState } from "preact/hooks";
import { api, User, WatchingPartner } from "../../api";
import { useAuth } from "../../context/auth";
import { GLOBAL_ALERT_EVENT } from "../../events";
import {
  formatDigestHour,
  utcMinutesToLocalHour,
  localHourToUtcMinutes,
} from "../../utils/digest";
import { formatDate } from "../../utils/time";
import "./style.css";
import { usePersistedState } from "../../hooks/usePersistedState";
import { sendToast } from "../../utils/toast";

export function Settings() {
  const { token } = useAuth();

  const [user, setUser] = useState<User | null>(null);
  const [watching, setWatching] = useState<WatchingPartner[]>([]);
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [nameStatus, setNameStatus] = useState<string | null>(null);
  const [savedButtonUntil, setSavedButtonUntil] = useState<number>(0);
  const [verificationLastSent, setVerificationLastSent] = usePersistedState<
    number | null
  >("verificationLastSent", null);
  const [emailFrequencyStatus, setEmailFrequencyStatus] = useState<
    string | null
  >(null);
  const [nameSaving, setNameSaving] = useState(false);
  const [verificationSending, setVerificationSending] = useState(false);
  const [emailFrequencySaving, setEmailFrequencySaving] = useState(false);
  const [emailDigestLocalHour, setEmailDigestLocalHour] = useState(6);
  const [emailScheduleStatus, setEmailScheduleStatus] = useState<string | null>(
    null,
  );
  const [emailScheduleSaving, setEmailScheduleSaving] = useState(false);

  const VERIFICATION_RESEND_COOLDOWN = 2 * 60 * 1000; // 2 minutes
  const verificationRecentlySent =
    +new Date() - verificationLastSent < VERIFICATION_RESEND_COOLDOWN;

  async function reload() {
    if (!token) return;
    const [nextUser, nextPartners] = await Promise.all([
      api.getUser(token),
      api.getPartners(token),
    ]);
    setUser(nextUser);
    setEmail(nextUser.email);
    setName(nextUser.name ?? "");
    setEmailDigestLocalHour(
      utcMinutesToLocalHour(nextUser.email_digest_minutes_utc),
    );
    setWatching(nextPartners.watching);
  }

  useEffect(() => {
    reload().catch(() => {});
  }, [token]);

  useEffect(() => {
    if (savedButtonUntil <= 0) return;
    const remaining = savedButtonUntil - Date.now();
    if (remaining <= 0) {
      setSavedButtonUntil(0);
      return;
    }
    const timer = window.setTimeout(() => {
      setSavedButtonUntil(0);
    }, remaining);
    return () => window.clearTimeout(timer);
  }, [savedButtonUntil]);

  const normalizedEmail = email.trim().toLowerCase();
  const trimmedName = name.trim();
  const profilePatch: {
    email?: string;
    name?: string;
  } = {};

  if (user && normalizedEmail !== user.email) {
    profilePatch.email = normalizedEmail;
  }

  if (user && trimmedName.length > 0 && trimmedName !== (user.name ?? "")) {
    profilePatch.name = trimmedName;
  }

  const hasProfileChanges = Object.keys(profilePatch).length > 0;
  const emailDigestMinutesUtc = localHourToUtcMinutes(emailDigestLocalHour);
  const hasDigestScheduleChanges = Boolean(
    user && emailDigestMinutesUtc !== user.email_digest_minutes_utc,
  );

  async function saveName(e: Event) {
    e.preventDefault();
    if (!token) return;
    if (!hasProfileChanges) {
      setNameStatus(null);
      return;
    }
    setNameStatus(null);
    setNameSaving(true);
    try {
      const emailChanged = Boolean(profilePatch.email);
      await api.updateUser(token, profilePatch);
      setSavedButtonUntil(Date.now() + 3000);
      setNameStatus(
        emailChanged
          ? "Profile saved. Please verify your new email address."
          : "Saved",
      );

      if (emailChanged) {
        setVerificationLastSent(null);
      }

      await reload();
    } catch (err) {
      setNameStatus(err instanceof Error ? err.message : "Failed to save");
    } finally {
      setNameSaving(false);
    }
  }

  async function resendVerificationEmail() {
    if (!token) return;
    setVerificationSending(true);
    try {
      const result = await api.requestVerificationEmail(token);
      sendToast(
        result.already_verified
          ? "Your email is already verified."
          : "Verification email sent.",
        result.already_verified,
      );
      setVerificationLastSent(Date.now());
      await reload();
    } catch (err) {
      sendToast(
        err instanceof Error
          ? err.message
          : "Failed to send verification email",
        true,
      );
    } finally {
      setVerificationSending(false);
    }
  }

  async function updateEmailFrequency(emailFrequency: User["email_frequency"]) {
    if (!token) return;
    setEmailFrequencyStatus(null);
    setEmailFrequencySaving(true);
    try {
      await api.updateUser(token, { email_frequency: emailFrequency });
      await reload();
      sendToast("Email preferences saved.");
    } catch (err) {
      setEmailFrequencyStatus(
        err instanceof Error ? err.message : "Failed to save",
      );
    } finally {
      setEmailFrequencySaving(false);
    }
  }

  async function saveEmailSchedule(e: Event) {
    e.preventDefault();
    if (!token || !user) return;

    setEmailScheduleStatus(null);

    setEmailScheduleSaving(true);
    try {
      await api.updateUser(token, {
        email_digest_minutes_utc: localHourToUtcMinutes(emailDigestLocalHour),
      });
      await reload();
      setEmailScheduleStatus("Digest schedule saved.");
    } catch (err) {
      setEmailScheduleStatus(
        err instanceof Error ? err.message : "Failed to save digest schedule",
      );
    } finally {
      setEmailScheduleSaving(false);
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
                setSavedButtonUntil(0);
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
                setSavedButtonUntil(0);
              }}
              placeholder="you@example.com"
              autoComplete="email"
              required
            />
          </div>
          {nameStatus && !nameStatus.toLowerCase().includes("saved") && (
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
          <button
            class="btn btn-primary"
            type="submit"
            disabled={nameSaving || !hasProfileChanges}
          >
            {nameSaving
              ? "Saving…"
              : savedButtonUntil > Date.now()
                ? "Saved"
                : "Save"}
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
                Your last verification email bounced on{" "}
                {formatDate(user.email_bounced_at)}. Update your email above
                before requesting another verification email.
              </p>
            )}
            <button
              class="btn btn-primary"
              type="button"
              disabled={
                verificationSending ||
                Boolean(user?.email_bounced_at) ||
                verificationRecentlySent
              }
              onClick={resendVerificationEmail}
            >
              {verificationSending
                ? "Sending…"
                : verificationRecentlySent
                  ? "Please wait 2 minutes before resending"
                  : "Resend verification email"}
            </button>
          </>
        )}
      </section>

      <section class="card settings-section">
        <h2>Email notifications</h2>
        <p class="settings-hint">
          Choose how often you receive accountability emails. If you monitor
          more than one person, each email includes one summary with a section
          for each person you monitor. Digests cover the 24 hours leading up to
          your chosen delivery time, converted from your current browser
          timezone.
        </p>
        <div class="field settings-frequency-field">
          <label for="settings-email-frequency">Email frequency</label>
          <select
            id="settings-email-frequency"
            class="settings-select"
            value={user?.email_frequency ?? "daily"}
            onChange={(e) =>
              updateEmailFrequency(
                (e.target as HTMLSelectElement)
                  .value as User["email_frequency"],
              ).catch(() => {})
            }
            disabled={!user || emailFrequencySaving}
          >
            <option value="none">None</option>
            <option value="alerts-only">Alerts only</option>
            <option value="daily">Daily</option>
            <option value="weekly">Weekly</option>
          </select>
        </div>
        {emailFrequencyStatus && (
          <p class="alert-error">{emailFrequencyStatus}</p>
        )}
        <form class="settings-form" onSubmit={saveEmailSchedule}>
          <div class="field settings-frequency-field">
            <label for="settings-email-digest-hour">Digest delivery time</label>
            <select
              id="settings-email-digest-hour"
              class="settings-select"
              value={String(emailDigestLocalHour)}
              onChange={(e) => {
                setEmailDigestLocalHour(
                  Number.parseInt((e.target as HTMLSelectElement).value, 10) ||
                    0,
                );
                setEmailScheduleStatus(null);
              }}
              disabled={!user || emailScheduleSaving}
            >
              {Array.from({ length: 24 }, (_, hour) => (
                <option key={hour} value={hour}>
                  {formatDigestHour(hour)}
                </option>
              ))}
            </select>
          </div>
          {emailScheduleStatus && (
            <p
              class={
                emailScheduleStatus.toLowerCase().includes("saved")
                  ? "alert-success"
                  : "alert-error"
              }
            >
              {emailScheduleStatus}
            </p>
          )}
          <button
            class="btn btn-primary"
            type="submit"
            disabled={!user || emailScheduleSaving || !hasDigestScheduleChanges}
          >
            {emailScheduleSaving ? "Saving…" : "Save digest schedule"}
          </button>
        </form>
        {watching.length === 0 ? (
          <p class="settings-hint settings-followup-hint">
            You are not monitoring anyone yet. This setting will apply once you
            accept a partner invite.
          </p>
        ) : (
          <p class="settings-hint settings-followup-hint">
            Currently monitoring{" "}
            {watching
              .map((partner) => partner.user.name ?? partner.user.email)
              .join(", ")}
            .
          </p>
        )}
      </section>
    </div>
  );
}
