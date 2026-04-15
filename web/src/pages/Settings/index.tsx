import { useEffect, useRef, useState } from "preact/hooks";
import { User } from "../../api";
import { useAuth } from "../../context/auth";
import { usePartners } from "../../hooks/usePartners";
import {
  formatDigestHour,
  utcMinutesToLocalHour,
  localHourToUtcMinutes,
} from "../../utils/digest";
import { formatDate } from "../../utils/time";
import "./style.css";
import { usePersistedState } from "../../hooks/usePersistedState";
import { useUser } from "../../hooks/useUser";
import { sendToast } from "../../utils/toast";

export function Settings() {
  const { token, logout } = useAuth();
  const {
    user,
    error: userError,
    isLoading: userLoading,
    updateUser,
    requestVerificationEmail,
    deleteUser,
  } = useUser();
  const {
    watching,
    error: partnersError,
    isLoading: partnersLoading,
  } = usePartners();

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
  const [deleteConfirmEmail, setDeleteConfirmEmail] = useState("");
  const [deleteAccountStatus, setDeleteAccountStatus] = useState<string | null>(
    null,
  );
  const [deleteAccountPending, setDeleteAccountPending] = useState(false);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  const VERIFICATION_RESEND_COOLDOWN = 2 * 60 * 1000; // 2 minutes
  const verificationRecentlySent =
    +new Date() - verificationLastSent < VERIFICATION_RESEND_COOLDOWN;
  const loadError = userError ?? partnersError;
  const settingsLoading = userLoading || partnersLoading;

  useEffect(() => {
    if (!user) {
      return;
    }

    setEmail(user.email);
    setName(user.name ?? "");
    setEmailDigestLocalHour(
      utcMinutesToLocalHour(user.email_digest_minutes_utc),
    );
  }, [user]);

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
  const deleteConfirmationMatches =
    Boolean(user) &&
    deleteConfirmEmail.trim().toLowerCase() === user.email.toLowerCase();

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
      await updateUser(profilePatch);
      setSavedButtonUntil(Date.now() + 3000);
      setNameStatus(
        emailChanged
          ? "Profile saved. Please verify your new email address."
          : "Saved",
      );

      if (emailChanged) {
        setVerificationLastSent(null);
      }
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
      const result = await requestVerificationEmail();
      sendToast(
        result.already_verified
          ? "Your email is already verified."
          : "Verification email sent.",
        result.already_verified,
      );
      setVerificationLastSent(Date.now());
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
      await updateUser({ email_frequency: emailFrequency });
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
      await updateUser({
        email_digest_minutes_utc: localHourToUtcMinutes(emailDigestLocalHour),
      });
      setEmailScheduleStatus("Digest schedule saved.");
    } catch (err) {
      setEmailScheduleStatus(
        err instanceof Error ? err.message : "Failed to save digest schedule",
      );
    } finally {
      setEmailScheduleSaving(false);
    }
  }

  function openDeleteDialog() {
    setDeleteConfirmEmail("");
    setDeleteAccountStatus(null);
    deleteDialogRef.current?.showModal();
  }

  function closeDeleteDialog() {
    if (deleteAccountPending) return;
    setDeleteConfirmEmail("");
    setDeleteAccountStatus(null);
    deleteDialogRef.current?.close();
  }

  async function deleteAccountConfirmed() {
    if (!token || !user || !deleteConfirmationMatches) {
      return;
    }

    setDeleteAccountStatus(null);
    setDeleteAccountPending(true);
    try {
      await deleteUser(user.email);
      if (typeof window !== "undefined") {
        window.sessionStorage.setItem(
          "virtue_global_link_message",
          JSON.stringify({
            message: "Your account has been deleted.",
            isError: false,
          }),
        );
      }
      deleteDialogRef.current?.close();
      await logout();
    } catch (err) {
      setDeleteAccountStatus(
        err instanceof Error ? err.message : "Failed to delete account",
      );
    } finally {
      setDeleteAccountPending(false);
    }
  }

  return (
    <div class="settings-page">
      <h1 class="settings-title">Settings</h1>
      {loadError && <p class="alert-error">{loadError.message}</p>}
      {settingsLoading && !user && !watching && (
        <p class="settings-hint">Loading…</p>
      )}

      {!settingsLoading && user && (
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
              disabled={userLoading || nameSaving || !hasProfileChanges}
            >
              {nameSaving
                ? "Saving…"
                : savedButtonUntil > Date.now()
                  ? "Saved"
                  : "Save"}
            </button>
          </form>
        </section>
      )}

      {!settingsLoading && user && (
        <section class="card settings-section">
          <h2>Email verification</h2>
          <p class="settings-hint">
            {user.email_verified
              ? `Your email (${user.email}) is verified.`
              : `Your email (${user.email}) is not verified yet.`}
          </p>
          {!user.email_verified && (
            <>
              {Boolean(user.email_bounced_at) && (
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
                  Boolean(user.email_bounced_at) ||
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
      )}

      {!settingsLoading && user && (
        <section class="card settings-section">
          <h2>Email notifications</h2>
          <p class="settings-hint">
            Choose how often you receive accountability emails. If you monitor
            more than one person, each email includes one summary with a section
            for each person you monitor. Digests cover the 24 hours leading up
            to your chosen delivery time, converted from your current browser
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
              <label for="settings-email-digest-hour">
                Digest delivery time
              </label>
              <select
                id="settings-email-digest-hour"
                class="settings-select"
                value={String(emailDigestLocalHour)}
                onChange={(e) => {
                  setEmailDigestLocalHour(
                    Number.parseInt(
                      (e.target as HTMLSelectElement).value,
                      10,
                    ) || 0,
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
              disabled={
                !user || emailScheduleSaving || !hasDigestScheduleChanges
              }
            >
              {emailScheduleSaving ? "Saving…" : "Save digest schedule"}
            </button>
          </form>
          {(watching ?? []).length === 0 ? (
            <p class="settings-hint settings-followup-hint">
              You are not monitoring anyone yet. This setting will apply once
              you accept a partner invite.
            </p>
          ) : (
            <p class="settings-hint settings-followup-hint">
              Currently monitoring{" "}
              {(watching ?? [])
                .map((partner) => partner.user.name ?? partner.user.email)
                .join(", ")}
              .
            </p>
          )}
        </section>
      )}

      {!settingsLoading && user && (
        <section class="card settings-section settings-danger-zone">
          <h2>Delete account</h2>
          <p class="settings-hint">
            This permanently deletes your account, devices, partner
            relationships, sessions, and stored logs. This cannot be undone.
          </p>
          <button
            class="btn btn-danger"
            type="button"
            onClick={openDeleteDialog}
            disabled={deleteAccountPending}
          >
            Delete account
          </button>
        </section>
      )}

      {!settingsLoading && user && (
        <dialog ref={deleteDialogRef} class="settings-dialog">
          <h3 class="dialog-title">Delete account</h3>
          <p class="invite-desc">
            This permanently removes your account and all associated data. Type{" "}
            <strong>{user.email}</strong> to confirm.
          </p>
          <div class="field">
            <label for="settings-delete-account-confirm-email">
              Confirm your email
            </label>
            <input
              id="settings-delete-account-confirm-email"
              type="email"
              value={deleteConfirmEmail}
              onInput={(e) => {
                setDeleteConfirmEmail((e.target as HTMLInputElement).value);
                setDeleteAccountStatus(null);
              }}
              placeholder={user.email}
              autoComplete="off"
              disabled={deleteAccountPending}
            />
          </div>
          {deleteAccountStatus && (
            <p class="alert-error">{deleteAccountStatus}</p>
          )}
          <div class="settings-dialog-actions">
            <button
              class="btn btn-danger"
              type="button"
              onClick={() => deleteAccountConfirmed().catch(() => {})}
              disabled={!deleteConfirmationMatches || deleteAccountPending}
            >
              {deleteAccountPending ? "Deleting…" : "Delete account"}
            </button>
            <button
              class="btn btn-ghost"
              type="button"
              onClick={closeDeleteDialog}
              disabled={deleteAccountPending}
            >
              Cancel
            </button>
          </div>
        </dialog>
      )}
    </div>
  );
}
