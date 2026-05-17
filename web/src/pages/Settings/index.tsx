import { useEffect, useRef, useState } from 'preact/hooks';
import { User, usePartners, useUser, useAPIContext } from '../../utils/api';
import { sendToast } from '../../utils/toast';
import {
  Alert,
  Button,
  Card,
  Dialog,
  DialogActions,
  DialogHeader,
  Field,
  Input,
  Select,
} from '@virtueinitiative/shared-web';
import { formatDigestHour, utcMinutesToLocalHour, localHourToUtcMinutes } from '../../utils/digest';
import './style.css';

export function Settings() {
  const api = useAPIContext();
  const user = useUser();
  const { watchings: watching } = usePartners();

  const [email, setEmail] = useState('');
  const [name, setName] = useState('');
  const [nameStatus, setNameStatus] = useState<string | null>(null);
  const [savedButtonUntil, setSavedButtonUntil] = useState<number>(0);
  const [profileSavePending, setProfileSavePending] = useState(false);
  const [emailScheduleSavedButtonUntil, setEmailScheduleSavedButtonUntil] = useState<number>(0);
  const [nameSaving, setNameSaving] = useState(false);
  const [emailFrequency, setEmailFrequency] = useState<User['email_frequency']>('daily');
  const [emailDigestLocalHour, setEmailDigestLocalHour] = useState(6);
  const [emailScheduleStatus, setEmailScheduleStatus] = useState<string | null>(null);
  const [emailScheduleSaving, setEmailScheduleSaving] = useState(false);
  const [deleteConfirmEmail, setDeleteConfirmEmail] = useState('');
  const [deleteAccountStatus, setDeleteAccountStatus] = useState<string | null>(null);
  const [deleteAccountPending, setDeleteAccountPending] = useState(false);
  const [emailChangeVerificationTarget, setEmailChangeVerificationTarget] = useState<string>('');
  const emailChangeDialogRef = useRef<HTMLDialogElement>(null);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  const settingsLoading = !user;

  useEffect(() => {
    if (!user) {
      return;
    }

    setEmail(user.email);
    setName(user.name ?? '');
    setEmailFrequency(user.email_frequency ?? 'daily');
    setEmailDigestLocalHour(utcMinutesToLocalHour(user.email_digest_minutes_utc));
  }, [user]);

  useEffect(() => {
    if (typeof window === 'undefined' || !api) return;
    const params = new URLSearchParams(window.location.search);
    const changeEmailToken = params.get('change_email_token');
    if (!changeEmailToken) return;

    params.delete('change_email_token');
    const cleanUrl = params.toString() ? `/settings?${params.toString()}` : '/settings';
    window.history.replaceState({}, '', cleanUrl);

    api
      .verifyEmailChange(changeEmailToken)
      .then(() => {
        sendToast('Email updated successfully.', { isError: false });
      })
      .catch((err: unknown) => {
        sendToast(err instanceof Error ? err.message : 'Failed to update email.', {
          isError: true,
        });
      });
  }, [api]);

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

  useEffect(() => {
    if (emailScheduleSavedButtonUntil <= 0) return;
    const remaining = emailScheduleSavedButtonUntil - Date.now();
    if (remaining <= 0) {
      setEmailScheduleSavedButtonUntil(0);
      return;
    }
    const timer = window.setTimeout(() => {
      setEmailScheduleSavedButtonUntil(0);
    }, remaining);
    return () => window.clearTimeout(timer);
  }, [emailScheduleSavedButtonUntil]);

  const normalizedEmail = email.trim().toLowerCase();
  const trimmedName = name.trim();
  const profilePatch: {
    email?: string;
    name?: string;
  } = {};

  if (user && normalizedEmail !== user.email) {
    profilePatch.email = normalizedEmail;
  }

  if (user && trimmedName.length > 0 && trimmedName !== (user.name ?? '')) {
    profilePatch.name = trimmedName;
  }

  const hasProfileChanges = Object.keys(profilePatch).length > 0;
  const emailDigestMinutesUtc = localHourToUtcMinutes(emailDigestLocalHour);
  const hasDigestScheduleChanges = Boolean(
    user && emailDigestMinutesUtc !== user.email_digest_minutes_utc,
  );
  const hasEmailFrequencyChanges = Boolean(user && emailFrequency !== user.email_frequency);
  const hasEmailScheduleChanges = hasDigestScheduleChanges || hasEmailFrequencyChanges;
  const deleteConfirmationMatches =
    Boolean(user) && deleteConfirmEmail.trim().toLowerCase() === user!.email.toLowerCase();

  async function saveName(e: Event) {
    e.preventDefault();
    if (!api) return;
    if (!hasProfileChanges) {
      setNameStatus(null);
      setProfileSavePending(false);
      return;
    }
    setNameStatus(null);
    setNameSaving(true);
    try {
      const emailChanged = Boolean(profilePatch.email);
      const result = await api.updateSettings(profilePatch);
      setSavedButtonUntil(Date.now() + 3000);
      if (emailChanged && result.email_verification_required) {
        setNameStatus(null);
        setProfileSavePending(true);
        setEmailChangeVerificationTarget(result.pending_email ?? normalizedEmail);
        emailChangeDialogRef.current?.showModal();
      } else {
        setProfileSavePending(false);
        setNameStatus('Saved');
      }
    } catch (err) {
      setNameStatus(err instanceof Error ? err.message : 'Failed to save');
    } finally {
      setNameSaving(false);
    }
  }

  async function saveEmailSchedule(e: Event) {
    e.preventDefault();
    if (!api || !user) return;
    if (!hasEmailScheduleChanges) {
      setEmailScheduleStatus(null);
      return;
    }

    setEmailScheduleStatus(null);

    const schedulePatch: {
      email_frequency?: User['email_frequency'];
      email_digest_minutes_utc?: User['email_digest_minutes_utc'];
    } = {};

    if (hasEmailFrequencyChanges) {
      schedulePatch.email_frequency = emailFrequency;
    }

    if (hasDigestScheduleChanges) {
      schedulePatch.email_digest_minutes_utc = emailDigestMinutesUtc;
    }

    setEmailScheduleSaving(true);
    try {
      await api.updateSettings(schedulePatch);
      setEmailScheduleSavedButtonUntil(Date.now() + 3000);
      setEmailScheduleStatus(null);
    } catch (err) {
      setEmailScheduleStatus(err instanceof Error ? err.message : 'Failed to save digest schedule');
    } finally {
      setEmailScheduleSaving(false);
    }
  }

  function openDeleteDialog() {
    setDeleteConfirmEmail('');
    setDeleteAccountStatus(null);
    deleteDialogRef.current?.showModal();
  }

  function closeDeleteDialog() {
    if (deleteAccountPending) return;
    setDeleteConfirmEmail('');
    setDeleteAccountStatus(null);
    deleteDialogRef.current?.close();
  }

  async function deleteAccountConfirmed() {
    if (!api || !user || !deleteConfirmationMatches) {
      return;
    }

    setDeleteAccountStatus(null);
    setDeleteAccountPending(true);
    try {
      await api.deleteUser(user.email);
      if (typeof window !== 'undefined') {
        window.sessionStorage.setItem(
          'virtue_global_link_message',
          JSON.stringify({
            message: 'Your account has been deleted.',
            isError: false,
          }),
        );
      }
      deleteDialogRef.current?.close();
      await api.logout();
    } catch (err) {
      setDeleteAccountStatus(err instanceof Error ? err.message : 'Failed to delete account');
    } finally {
      setDeleteAccountPending(false);
    }
  }

  return (
    <div class="settings-page">
      <h1 class="settings-title">Settings</h1>
      {settingsLoading && <p class="hint-text">Loading…</p>}

      {!settingsLoading && user && (
        <Card class="settings-section">
          <h2>Profile</h2>
          <form class="settings-form" onSubmit={saveName}>
            <Field label="Display name">
              <Input
                type="text"
                value={name}
                onInput={(e) => {
                  setName((e.target as HTMLInputElement).value);
                  setNameStatus(null);
                  setSavedButtonUntil(0);
                  setProfileSavePending(false);
                }}
                placeholder="Your name"
                autoComplete="name"
              />
            </Field>
            <Field label="Email">
              <Input
                type="email"
                value={email}
                onInput={(e) => {
                  setEmail((e.target as HTMLInputElement).value);
                  setNameStatus(null);
                  setSavedButtonUntil(0);
                  setProfileSavePending(false);
                }}
                placeholder="you@example.com"
                autoComplete="email"
                required
              />
            </Field>
            {nameStatus && (
              <Alert variant={nameStatus.toLowerCase().includes('saved') ? 'success' : 'error'}>
                {nameStatus}
              </Alert>
            )}
            <Button
              variant="primary"
              type="submit"
              disabled={!api || nameSaving || profileSavePending || !hasProfileChanges}
            >
              {nameSaving
                ? 'Saving…'
                : profileSavePending
                  ? 'Pending'
                  : savedButtonUntil > Date.now()
                    ? 'Saved'
                    : 'Save'}
            </Button>
          </form>
          <Dialog dialogRef={emailChangeDialogRef} class="settings-dialog">
            <DialogHeader>Confirm your new email</DialogHeader>
            <p class="invite-desc">
              We sent a verification link to{' '}
              <strong>{emailChangeVerificationTarget || 'your new email'}</strong>. Your account
              email will change after you confirm that link.
            </p>
            <DialogActions>
              <Button
                variant="primary"
                type="button"
                onClick={() => emailChangeDialogRef.current?.close()}
              >
                Got it
              </Button>
            </DialogActions>
          </Dialog>
        </Card>
      )}

      {!settingsLoading && user && (
        <Card class="settings-section">
          <h2>Email notifications</h2>
          <p class="hint-text settings-section-hint">
            Choose how often and when you receive reminders to review your partners' screenshots
          </p>
          <Field label="Email frequency">
            <Select
              value={emailFrequency}
              onChange={(e) => {
                setEmailFrequency((e.target as HTMLSelectElement).value as User['email_frequency']);
                setEmailScheduleStatus(null);
                setEmailScheduleSavedButtonUntil(0);
              }}
              disabled={!user || emailScheduleSaving}
            >
              <option value="none">None</option>
              <option value="alerts-only">Alerts only</option>
              <option value="daily">Daily</option>
              <option value="weekly">Weekly</option>
            </Select>
          </Field>
          <form class="settings-form" onSubmit={saveEmailSchedule}>
            <Field label="Digest delivery time">
              <Select
                value={String(emailDigestLocalHour)}
                onChange={(e) => {
                  setEmailDigestLocalHour(
                    Number.parseInt((e.target as HTMLSelectElement).value, 10) || 0,
                  );
                  setEmailScheduleStatus(null);
                  setEmailScheduleSavedButtonUntil(0);
                }}
                disabled={!user || emailScheduleSaving}
              >
                {Array.from({ length: 24 }, (_, hour) => (
                  <option key={hour} value={hour}>
                    {formatDigestHour(hour)}
                  </option>
                ))}
              </Select>
            </Field>
            {emailScheduleStatus && <Alert variant="error">{emailScheduleStatus}</Alert>}
            <Button
              variant="primary"
              type="submit"
              disabled={!user || emailScheduleSaving || !hasEmailScheduleChanges}
            >
              {emailScheduleSaving
                ? 'Saving…'
                : emailScheduleSavedButtonUntil > Date.now()
                  ? 'Saved'
                  : 'Save digest schedule'}
            </Button>
          </form>
          {watching.length === 0 ? (
            <p class="hint-text settings-followup-hint">
              You are not monitoring anyone yet. This setting will apply once you accept a partner
              invite.
            </p>
          ) : (
            <p class="hint-text settings-followup-hint">
              Currently monitoring{' '}
              {watching.map((partner) => partner.user.name ?? partner.user.email).join(', ')}.
            </p>
          )}
        </Card>
      )}

      {!settingsLoading && user && (
        <Card class="settings-section settings-danger-zone">
          <h2>Delete account</h2>
          <p class="hint-text settings-section-hint">
            This permanently deletes your account, devices, partner relationships, sessions, and
            stored logs. This cannot be undone.
          </p>
          <Button
            variant="danger"
            type="button"
            onClick={openDeleteDialog}
            disabled={deleteAccountPending}
          >
            Delete account
          </Button>
        </Card>
      )}

      {!settingsLoading && user && (
        <Dialog dialogRef={deleteDialogRef} class="settings-dialog">
          <DialogHeader>Delete account</DialogHeader>
          <p class="invite-desc">
            This permanently removes your account and all associated data. Type{' '}
            <strong>{user.email}</strong> to confirm.
          </p>
          <Field label="Confirm your email">
            <Input
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
          </Field>
          {deleteAccountStatus && <Alert variant="error">{deleteAccountStatus}</Alert>}
          <DialogActions>
            <Button
              variant="ghost"
              type="button"
              onClick={closeDeleteDialog}
              disabled={deleteAccountPending}
            >
              Cancel
            </Button>
            <Button
              variant="danger"
              type="button"
              onClick={() => deleteAccountConfirmed().catch(() => {})}
              disabled={!deleteConfirmationMatches || deleteAccountPending}
            >
              {deleteAccountPending ? 'Deleting…' : 'Delete account'}
            </Button>
          </DialogActions>
        </Dialog>
      )}
    </div>
  );
}
