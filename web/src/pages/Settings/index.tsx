import { useEffect, useRef, useState } from 'preact/hooks';
import { User, useUser, useAPIContext, describeError } from '../../utils/api';
import { PageHeading } from '../../components/PageHeading';
import { SettingsIcon } from '../../components/icons';
import { ClearCacheButton } from '../../components/ClearCacheButton';
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
  Switch,
} from '@virtueinitiative/shared-web';
import { dnsFilterDomainSchema } from '@virtueinitiative/shared-web/types';
import { usePromise } from '../../hooks/usePromise';
import { usePwnedPasswordCount } from '../../hooks/usePwnedPasswordCount';
import { PasswordField, PwnedPasswordWarning } from '../../components/PasswordField';
import { MIN_PASSWORD_LENGTH, passwordLengthError } from '../../utils/password-policy';
import './style.css';

export function Settings() {
  const api = useAPIContext();
  const user = useUser();

  const [email, setEmail] = useState('');
  const [name, setName] = useState('');
  const [settingsStatus, setSettingsStatus] = useState<string | null>(null);
  const [settingsSaving, setSettingsSave] = usePromise();
  const [settingsShowSaved, setSettingsShowSaved] = useState(false);
  const [dnsOn, setDnsOn] = useState(false);
  const [dnsDomain, setDnsDomain] = useState('');
  const [dnsError, setDnsError] = useState<string | undefined>();
  const [dnsSaving, setDnsSave] = usePromise();
  const [dnsShowSaved, setDnsShowSaved] = useState(false);
  const [dnsOffSaved, setDnsOffSaved] = useState(false);
  const [dnsOffError, setDnsOffError] = useState<string | null>(null);
  const focusDnsField = useRef(false);
  const [emailStatus, setEmailStatus] = useState<string | null>(null);
  const [emailSaving, setEmailSave] = usePromise();
  const [emailFrequency, setEmailFrequency] =
    useState<User['settings']['email_frequency']>('daily');
  const [deleteConfirmEmail, setDeleteConfirmEmail] = useState('');
  const [deleteAccountStatus, setDeleteAccountStatus] = useState<string | null>(null);
  const [deleteAccountPending, setDeleteAccountSave] = usePromise();
  const [emailChangeVerificationTarget, setEmailChangeVerificationTarget] = useState<string>('');
  const [currentPassword, setCurrentPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [passwordStatus, setPasswordStatus] = useState<string | null>(null);
  const [passwordChanged, setPasswordChanged] = useState(false);
  const [passwordSaving, setPasswordSave] = usePromise();
  const pwnedCount = usePwnedPasswordCount(newPassword, true);
  const emailChangeDialogRef = useRef<HTMLDialogElement>(null);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);
  const dnsOffDialogRef = useRef<HTMLDialogElement>(null);
  const dnsChangeDialogRef = useRef<HTMLDialogElement>(null);
  const pendingDnsDomain = useRef('');

  useEffect(() => {
    if (!user) return;
    setEmail(user.email);
    setName(user.name ?? '');
    setEmailFrequency(user.settings.email_frequency ?? 'daily');
    setDnsDomain(user.dns_filter_domain ?? '');
    // A saved domain means the check is on. Keep an unsaved switch-on across refetches.
    setDnsOn((prev) => !!user.dns_filter_domain || prev);
  }, [user]);

  useEffect(() => {
    if (dnsOn && focusDnsField.current) {
      focusDnsField.current = false;
      document.getElementById('dns-check-domain')?.focus();
    }
  }, [dnsOn]);

  if (!user) return <p class="hint-text">Loading…</p>;

  const normalizedEmail = email.trim().toLowerCase();
  const trimmedName = name.trim();

  const hasNameChange = trimmedName.length > 0 && trimmedName !== (user.name ?? '');
  const hasEmailChanged = normalizedEmail !== user.email;
  const hasEmailFrequencyChanges = emailFrequency !== user.settings.email_frequency;
  const hasSettingsChanges = hasNameChange || hasEmailFrequencyChanges;
  // With nothing saved yet, Save stays available so an empty field can explain what is missing.
  const hasDnsDomainChange =
    !user.dns_filter_domain || dnsDomain.trim().toLowerCase() !== user.dns_filter_domain;

  const deleteConfirmationMatches =
    deleteConfirmEmail.trim().toLowerCase() === user.email.toLowerCase();

  function saveSettings(e: Event) {
    e.preventDefault();
    if (!api || !hasSettingsChanges) return;
    setSettingsStatus(null);
    const patch: Parameters<typeof api.updateSettings>[0] = {};
    if (hasNameChange) patch.name = trimmedName;
    if (hasEmailFrequencyChanges) patch.settings = { email_frequency: emailFrequency };
    setSettingsSave(() =>
      api
        .updateSettings(patch)
        .then(() => setSettingsShowSaved(true))
        .catch((err: unknown) => {
          setSettingsStatus(describeError(err, 'Failed to save'));
        }),
    );
  }

  // WEB-073: switching on only reveals the field. Nothing is sent until the user saves.
  // Switching off with a saved domain asks first and leaves the switch on until confirmed.
  function toggleDnsCheck(next: boolean) {
    setDnsOffSaved(false);
    setDnsOffError(null);
    setDnsError(undefined);
    setDnsShowSaved(false);
    if (next) {
      focusDnsField.current = true;
      setDnsOn(true);
      return;
    }
    if (api && user?.dns_filter_domain) {
      dnsOffDialogRef.current?.showModal();
      return;
    }
    setDnsOn(false);
    setDnsDomain('');
  }

  function confirmDnsOff() {
    dnsOffDialogRef.current?.close();
    const saved = user?.dns_filter_domain ?? '';
    setDnsOn(false);
    setDnsDomain('');
    if (!api || !saved) return;
    setDnsSave(() =>
      api
        .updateSettings({ dns_filter_domain: null })
        .then(() => setDnsOffSaved(true))
        .catch((err: unknown) => {
          setDnsOn(true);
          setDnsDomain(saved);
          setDnsOffError(describeError(err, 'Failed to turn off the DNS filter check.'));
        }),
    );
  }

  // WEB-073: the domain must be valid. An empty field is an error, not "off".
  // Replacing a saved domain asks first, so the user can block the new one before it is checked.
  function saveDnsDomain(e: Event) {
    e.preventDefault();
    if (!api || !hasDnsDomainChange) return;
    const parsed = dnsFilterDomainSchema.safeParse(dnsDomain);
    if (!parsed.success) {
      setDnsError(parsed.error.issues[0]?.message ?? 'Enter a domain such as example.net.');
      return;
    }
    setDnsError(undefined);
    pendingDnsDomain.current = parsed.data;
    if (user?.dns_filter_domain) {
      dnsChangeDialogRef.current?.showModal();
      return;
    }
    sendDnsDomain();
  }

  function sendDnsDomain() {
    if (!api) return;
    setDnsSave(() =>
      api
        .updateSettings({ dns_filter_domain: pendingDnsDomain.current })
        .then(() => setDnsShowSaved(true))
        .catch((err: unknown) => {
          setDnsError(describeError(err, 'Failed to save') ?? undefined);
        }),
    );
  }

  function confirmDnsChange() {
    dnsChangeDialogRef.current?.close();
    sendDnsDomain();
  }

  function saveEmail(e: Event) {
    e.preventDefault();
    if (!api || !hasEmailChanged) return;
    setEmailStatus(null);
    setEmailSave(() =>
      api
        .updateSettings({ email: normalizedEmail })
        .then((result) => {
          if (result.email_bounced) {
            setEmailStatus(
              `Email to ${result.pending_email ?? normalizedEmail} bounced, so we did not send a verification link. Use a different email address.`,
            );
          } else if (result.email_verification_required) {
            setEmailChangeVerificationTarget(result.pending_email ?? normalizedEmail);
            emailChangeDialogRef.current?.showModal();
          } else {
            setSettingsShowSaved(true);
          }
        })
        .catch((err: unknown) => {
          setEmailStatus(describeError(err, 'Failed to update email'));
        }),
    );
  }

  function onPasswordInput(setter: (value: string) => void) {
    return (e: Event) => {
      setter((e.target as HTMLInputElement).value);
      setPasswordStatus(null);
      setPasswordChanged(false);
    };
  }

  function changePassword(e: Event) {
    e.preventDefault();
    if (!api) return;
    setPasswordStatus(null);
    setPasswordChanged(false);
    const problem =
      newPassword !== confirmPassword
        ? 'Passwords do not match.'
        : newPassword === currentPassword
          ? 'Choose a new password that is different from your current one.'
          : passwordLengthError(newPassword);
    if (problem) {
      setPasswordStatus(problem);
      return;
    }
    setPasswordSave(() =>
      api
        .changePassword(currentPassword, newPassword)
        .then(() => {
          setCurrentPassword('');
          setNewPassword('');
          setConfirmPassword('');
          setPasswordChanged(true);
        })
        .catch((err: unknown) => {
          setPasswordStatus(describeError(err, 'Failed to change password'));
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
    setDeleteAccountSave(() =>
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
          setDeleteAccountStatus(describeError(err, 'Failed to delete account'));
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

      <Card class="settings-section">
        <h2>Password</h2>
        <p class="hint-text settings-section-hint">
          Your existing logs stay readable after you change your password. Other browsers signed in
          to your account are logged out. Your devices stay signed in and keep monitoring.
        </p>
        <form class="settings-form" onSubmit={changePassword}>
          {/* Lets password managers attach the new password to the right account. */}
          <input type="email" value={user.email} autoComplete="username" readOnly hidden />
          <PasswordField
            label="Current password"
            id="current-password"
            name="current-password"
            value={currentPassword}
            onInput={onPasswordInput(setCurrentPassword)}
            autoComplete="current-password"
            required
            disabled={passwordSaving}
          />
          <PasswordField
            label="New password"
            id="new-password"
            name="new-password"
            value={newPassword}
            onInput={onPasswordInput(setNewPassword)}
            autoComplete="new-password"
            required
            disabled={passwordSaving}
            helpText={`Use at least ${MIN_PASSWORD_LENGTH} characters.`}
            error={
              newPassword.length > 0 && newPassword.length < MIN_PASSWORD_LENGTH
                ? (passwordLengthError(newPassword) ?? undefined)
                : undefined
            }
          />
          <PwnedPasswordWarning count={pwnedCount} />
          <PasswordField
            label="Confirm new password"
            id="confirm-new-password"
            name="confirm-new-password"
            value={confirmPassword}
            onInput={onPasswordInput(setConfirmPassword)}
            autoComplete="new-password"
            required
            disabled={passwordSaving}
          />
          {passwordStatus && <Alert variant="error">{passwordStatus}</Alert>}
          {passwordChanged && <Alert variant="success">Your password was changed.</Alert>}
          <Button
            variant="primary"
            type="submit"
            disabled={
              !api || passwordSaving || !currentPassword || !newPassword || !confirmPassword
            }
          >
            {passwordSaving ? 'Changing password…' : 'Change password'}
          </Button>
        </form>
      </Card>

      <Card class="settings-section">
        <h2>DNS filter check</h2>
        <p class="hint-text settings-section-hint">
          Add a domain to your DNS filter's block list, then enter it here. All your devices check
          that it stays blocked. Virtue alerts your partners if it stops being blocked, and notifies
          them when you turn the check off.
        </p>
        <div class="settings-form">
          <Switch
            label="Check my DNS filter"
            checked={dnsOn}
            onChange={toggleDnsCheck}
            disabled={!api || dnsSaving}
          />
          {dnsOffSaved && (
            <p class="hint-text" role="status">
              Saved
            </p>
          )}
          {dnsOffError && <Alert variant="error">{dnsOffError}</Alert>}
        </div>
        {dnsOn && (
          <form class="settings-form" onSubmit={saveDnsDomain}>
            <Field label="DNS check domain" id="dns-check-domain" error={dnsError}>
              <Input
                id="dns-check-domain"
                type="text"
                inputMode="url"
                autoCapitalize="none"
                autoCorrect="off"
                spellcheck={false}
                placeholder="example.net"
                value={dnsDomain}
                error={!!dnsError}
                aria-invalid={dnsError ? true : undefined}
                onInput={(e) => {
                  setDnsDomain((e.target as HTMLInputElement).value);
                  setDnsError(undefined);
                  setDnsShowSaved(false);
                }}
                disabled={dnsSaving}
              />
            </Field>
            <Button
              variant="primary"
              type="submit"
              disabled={!api || dnsSaving || !hasDnsDomainChange}
            >
              {dnsSaving ? 'Saving…' : dnsShowSaved ? 'Saved' : 'Save'}
            </Button>
          </form>
        )}
        <Dialog dialogRef={dnsOffDialogRef} class="settings-dialog">
          <DialogHeader>Turn off the DNS filter check?</DialogHeader>
          <p class="invite-desc">
            This check is here to protect you. If you turn it off, your partners are notified. Talk
            with them first if you can.
          </p>
          <DialogActions>
            <Button
              variant="ghost"
              type="button"
              autoFocus
              onClick={() => dnsOffDialogRef.current?.close()}
            >
              Keep it on
            </Button>
            <Button variant="danger" type="button" onClick={confirmDnsOff}>
              Turn off
            </Button>
          </DialogActions>
        </Dialog>

        <Dialog dialogRef={dnsChangeDialogRef} class="settings-dialog">
          <DialogHeader>Change the DNS check domain?</DialogHeader>
          <p class="invite-desc">
            Add the new domain to your DNS filter's block list first. If it is not blocked, your
            devices report the filter as off and your partners get an alert.
          </p>
          <DialogActions>
            <Button
              variant="ghost"
              type="button"
              autoFocus
              onClick={() => dnsChangeDialogRef.current?.close()}
            >
              Cancel
            </Button>
            <Button variant="primary" type="button" onClick={confirmDnsChange}>
              Change domain
            </Button>
          </DialogActions>
        </Dialog>
      </Card>

      <Card class="settings-section">
        <h2>Local cache</h2>
        <p class="hint-text settings-section-hint">
          Your logs are decrypted and stored on this device so they load quickly. Clear the cache if
          logs are missing or fail to load. The app downloads and decrypts them again the next time
          you open your logs.
        </p>
        <ClearCacheButton />
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

      <Dialog dialogRef={deleteDialogRef} class="settings-dialog" busy={deleteAccountPending}>
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
