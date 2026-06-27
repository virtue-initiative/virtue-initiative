import { useEffect, useRef, useState } from 'preact/hooks';
import { User, useUser, useAPIContext } from '../../utils/api';
import { PageHeading } from '../../components/PageHeading';
import { SettingsIcon } from '../../components/icons';
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
import { usePromise } from '../../hooks/usePromise';
import './style.css';

export function Settings() {
  const api = useAPIContext();
  const user = useUser();

  const [email, setEmail] = useState('');
  const [name, setName] = useState('');
  const [settingsStatus, setSettingsStatus] = useState<string | null>(null);
  const [settingsSaving, setSettingsSave] = usePromise();
  const [settingsShowSaved, setSettingsShowSaved] = useState(false);
  const [emailStatus, setEmailStatus] = useState<string | null>(null);
  const [emailSaving, setEmailSave] = usePromise();
  const [emailFrequency, setEmailFrequency] =
    useState<User['settings']['email_frequency']>('daily');
  const [deleteConfirmEmail, setDeleteConfirmEmail] = useState('');
  const [deleteAccountStatus, setDeleteAccountStatus] = useState<string | null>(null);
  const [deleteAccountPending, setDeleteAccountSave] = usePromise();
  const [emailChangeVerificationTarget, setEmailChangeVerificationTarget] = useState<string>('');
  const emailChangeDialogRef = useRef<HTMLDialogElement>(null);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    if (!user) return;
    setEmail(user.email);
    setName(user.name ?? '');
    setEmailFrequency(user.settings.email_frequency ?? 'daily');
  }, [user]);

  if (!user) return <p class="hint-text">Loading…</p>;

  const normalizedEmail = email.trim().toLowerCase();
  const trimmedName = name.trim();

  const hasNameChange = trimmedName.length > 0 && trimmedName !== (user.name ?? '');
  const hasEmailChanged = normalizedEmail !== user.email;
  const hasEmailFrequencyChanges = emailFrequency !== user.settings.email_frequency;
  const hasSettingsChanges = hasNameChange || hasEmailFrequencyChanges;

  const deleteConfirmationMatches =
    deleteConfirmEmail.trim().toLowerCase() === user.email.toLowerCase();

  function saveSettings(e: Event) {
    e.preventDefault();
    if (!api || !hasSettingsChanges) return;
    setSettingsStatus(null);
    const patch: Parameters<typeof api.updateSettings>[0] = {};
    if (hasNameChange) patch.name = trimmedName;
    if (hasEmailFrequencyChanges) patch.settings = { email_frequency: emailFrequency };
    setSettingsSave(
      api
        .updateSettings(patch)
        .then(() => setSettingsShowSaved(true))
        .catch((err: unknown) => {
          setSettingsStatus(err instanceof Error ? err.message : 'Failed to save');
        }),
    );
  }

  function saveEmail(e: Event) {
    e.preventDefault();
    if (!api || !hasEmailChanged) return;
    setEmailStatus(null);
    setEmailSave(
      api
        .updateSettings({ email: normalizedEmail })
        .then((result) => {
          if (result.email_verification_required) {
            setEmailChangeVerificationTarget(result.pending_email ?? normalizedEmail);
            emailChangeDialogRef.current?.showModal();
          } else {
            setSettingsShowSaved(true);
          }
        })
        .catch((err: unknown) => {
          setEmailStatus(err instanceof Error ? err.message : 'Failed to update email');
        }),
    );
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

  function deleteAccountConfirmed() {
    if (!api || !deleteConfirmationMatches) return;
    setDeleteAccountStatus(null);
    setDeleteAccountSave(
      api
        .deleteUser(user.email)
        .then(async () => {
          if (typeof window !== 'undefined') {
            window.sessionStorage.setItem(
              'virtue_global_link_message',
              JSON.stringify({ message: 'Your account has been deleted.', isError: false }),
            );
          }
          deleteDialogRef.current?.close();
          await api.logout();
        })
        .catch((err: unknown) => {
          setDeleteAccountStatus(err instanceof Error ? err.message : 'Failed to delete account');
        }),
    );
  }

  return (
    <div class="settings-page">
      <PageHeading icon={<SettingsIcon />}>Settings</PageHeading>

      <Card class="settings-section">
        <form class="settings-form" onSubmit={saveSettings}>
          <Field label="Display name">
            <Input
              type="text"
              value={name}
              onInput={(e) => {
                setName((e.target as HTMLInputElement).value);
                setSettingsStatus(null);
                setSettingsShowSaved(false);
              }}
              placeholder="Your name"
              autoComplete="name"
            />
          </Field>
          <Field label="Email frequency">
            <Select
              value={emailFrequency}
              onChange={(e) => {
                setEmailFrequency(
                  (e.target as HTMLSelectElement).value as User['settings']['email_frequency'],
                );
                setSettingsStatus(null);
                setSettingsShowSaved(false);
              }}
              disabled={settingsSaving}
            >
              <option value="none">None</option>
              <option value="alerts-only">Alerts only</option>
              <option value="daily">Daily</option>
              <option value="weekly">Weekly</option>
            </Select>
          </Field>
          {settingsStatus && <Alert variant="error">{settingsStatus}</Alert>}
          <Button
            variant="primary"
            type="submit"
            disabled={!api || settingsSaving || !hasSettingsChanges}
          >
            {settingsSaving ? 'Saving…' : settingsShowSaved ? 'Saved' : 'Save'}
          </Button>
        </form>
      </Card>

      <Card class="settings-section">
        <form class="settings-form" onSubmit={saveEmail}>
          <Field label="Email">
            <Input
              type="email"
              value={email}
              onInput={(e) => {
                setEmail((e.target as HTMLInputElement).value);
                setEmailStatus(null);
              }}
              placeholder="you@example.com"
              autoComplete="email"
              required
            />
          </Field>
          {emailStatus && <Alert variant="error">{emailStatus}</Alert>}
          <Button
            variant="primary"
            type="submit"
            disabled={!api || emailSaving || !hasEmailChanged}
          >
            {emailSaving ? 'Sending verification email…' : 'Verify'}
          </Button>
        </form>

        <Dialog dialogRef={emailChangeDialogRef} class="settings-dialog">
          <DialogHeader>Confirm your new email</DialogHeader>
          <p class="invite-desc">
            We sent a verification link to{' '}
            <strong>{emailChangeVerificationTarget || 'your new email'}</strong>. Your account email
            will change after you confirm that link.
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

      <Card class="settings-section settings-danger-zone">
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
            onClick={deleteAccountConfirmed}
            disabled={!deleteConfirmationMatches || deleteAccountPending}
          >
            {deleteAccountPending ? 'Deleting…' : 'Delete account'}
          </Button>
        </DialogActions>
      </Dialog>
    </div>
  );
}
