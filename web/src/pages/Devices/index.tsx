import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { useLocation } from 'preact-iso';
import {
  Device,
  DeviceCodeLookupResponse,
  describeError,
  useAPIContext,
  useDevices,
} from '../../utils/api';
import { PageHeading } from '../../components/PageHeading';
import { DeviceStatusBadge } from '../../components/DeviceStatusBadge';
import { DevicesIcon } from '../../components/icons';
import {
  Badge,
  Button,
  Card,
  CardActions,
  CardGrid,
  CardHeader,
  Dialog,
  DialogActions,
  DialogHeader,
  DialogSecondaryActions,
  Field,
  Input,
  useToast,
} from '@virtueinitiative/shared-web';
import { formatRelativeTimestamp } from '../../utils/time';
import { LANDING_URL } from '../../utils/landing-url';
import './style.css';

const DOWNLOAD_URL = `${LANDING_URL}/download`;

/** The characters a pairing code can contain, in the order they were given. */
const codeCharsOf = (value: string) => value.toUpperCase().replace(/[^0-9A-Z]/g, '');

/** `K7RM3X` -> `K7R-M3X`, leaving a partial code alone until it needs the dash. */
const formatCode = (chars: string) =>
  chars.length > 3 ? `${chars.slice(0, 3)}-${chars.slice(3)}` : chars;

export function Devices() {
  const api = useAPIContext();
  const userId = api?.userId ?? null;
  const { devices, loaded } = useDevices();
  const updateDevice = (id: string, patch: { name?: string }) =>
    api ? api.updateDevice(id, patch) : Promise.reject(new Error('Not signed in'));
  const removeDevice = (id: string) =>
    api ? api.removeDevice(id) : Promise.reject(new Error('Not signed in'));

  const ownDevices = useMemo(
    () => devices.filter((device) => device.owner === userId),
    [devices, userId],
  );

  return (
    <div class="dashboard">
      <PageHeading icon={<DevicesIcon />} actions={<AddDeviceButton />}>
        Devices
      </PageHeading>
      {!loaded ? (
        <p class="loading">Loading…</p>
      ) : ownDevices.length === 0 ? (
        <p class="empty">No devices</p>
      ) : (
        <CardGrid>
          {ownDevices.map((device) => (
            <DeviceCard
              key={device.id}
              device={device}
              onUpdateDevice={updateDevice}
              onRemoveDevice={removeDevice}
            />
          ))}
        </CardGrid>
      )}
    </div>
  );
}

/**
 * API-043's web half. Two steps on purpose: typing the code only resolves it to
 * a device name and platform, and a second, explicit Add is what actually signs
 * that device in. A phished code is therefore visible as an unfamiliar device
 * before it is honored.
 */
function AddDeviceButton() {
  const api = useAPIContext();
  const dialogRef = useRef<HTMLDialogElement>(null);
  // Held formatted (`K7R-M3X`); `userCode` below is the bare six characters.
  const [code, setCode] = useState('');
  const [pending, setPending] = useState<DeviceCodeLookupResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { push: pushToast } = useToast();
  // The clients print a `/devices?add=<code>` link, so following it should land
  // on the code box with the code already in it, not on a page with a button
  // still to find and a code still to copy across.
  const { url, path, route } = useLocation();
  const autoOpened = useRef(false);

  useEffect(() => {
    const query = url.includes('?') ? url.slice(url.indexOf('?')) : '';
    const params = new URLSearchParams(query);
    if (!params.has('add') || autoOpened.current) return;
    autoOpened.current = true;
    open(params.get('add') ?? '');
  }, [url]);

  const userCode = code.replace('-', '');

  function reset() {
    setCode('');
    setPending(null);
    setError(null);
    setLoading(false);
  }

  /**
   * `prefill` is whatever `?add=` carried. Anything but a complete code is
   * ignored rather than half-filled: a truncated code would look like the user
   * mistyped it, and the box is easier to use empty than partly wrong.
   */
  function open(prefill = '') {
    reset();
    const chars = codeCharsOf(prefill);
    if (chars.length === 6) setCode(formatCode(chars));
    clearAddParam();
    dialogRef.current?.showModal();
  }

  /**
   * Drops `?add` once it has done its job, so reloading the page or coming back
   * to it later does not reopen the dialog over whatever the user is doing.
   * Replaces the history entry rather than pushing one, so Back still leaves
   * the page instead of landing on the link again.
   */
  function clearAddParam() {
    const queryIndex = url.indexOf('?');
    if (queryIndex === -1) return;
    const params = new URLSearchParams(url.slice(queryIndex));
    if (!params.has('add')) return;
    params.delete('add');
    const rest = params.toString();
    route(rest ? `${path}?${rest}` : path, true);
  }

  function close() {
    dialogRef.current?.close();
  }

  /**
   * Keeps the box showing `XXX-XXX` however the code arrives: typed without the
   * dash, pasted with one, lowercase, or spaced. Characters outside the code
   * alphabet are dropped rather than rejected, which is how the server
   * normalizes too (API-046), so a pasted `k7r m3x` just works.
   *
   * Reformatting inserts and drops characters, so the caret is remapped by how
   * many code characters precede it rather than restored to its raw offset.
   */
  function handleCodeInput(input: HTMLInputElement, backspaced: boolean) {
    const raw = input.value;
    const caret = input.selectionStart ?? raw.length;
    let cleaned = codeCharsOf(raw).slice(0, 6);
    let precedingChars = Math.min(codeCharsOf(raw.slice(0, caret)).length, cleaned.length);

    // Backspace over the dash deletes a character the user never typed, so
    // reformatting would put it straight back and the key would look dead.
    // Take the code character before it instead.
    if (backspaced && cleaned === userCode && precedingChars > 0) {
      cleaned = cleaned.slice(0, precedingChars - 1) + cleaned.slice(precedingChars);
      precedingChars -= 1;
    }

    const formatted = formatCode(cleaned);
    const caretAfter = Math.min(
      precedingChars > 3 ? precedingChars + 1 : precedingChars,
      formatted.length,
    );

    // Written straight to the DOM as well as to state: a keystroke that changes
    // nothing (a rejected character, say) re-renders nothing, so state alone
    // would leave the raw text in the box. Doing it here also means the render
    // that follows finds the value already correct and leaves the caret alone.
    input.value = formatted;
    input.setSelectionRange(caretAfter, caretAfter);
    setCode(formatted);
  }

  async function handleLookup(e: Event) {
    e.preventDefault();
    if (!api) return;
    setLoading(true);
    setError(null);
    try {
      setPending(await api.lookupDeviceCode(userCode));
    } catch (err) {
      setError(describeError(err, 'That code is not valid. It may have expired.'));
    } finally {
      setLoading(false);
    }
  }

  async function handleApprove() {
    if (!api) return;
    setLoading(true);
    setError(null);
    try {
      const approved = await api.approveDeviceCode(userCode);
      close();
      pushToast(`${approved.name} was added. It will appear here shortly.`, 'success');
    } catch (err) {
      setError(describeError(err, 'That code could not be approved.'));
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <Button variant="primary" type="button" onClick={() => open()}>
        Add device
      </Button>
      <Dialog dialogRef={dialogRef} class="device-setup-dialog" onClose={clearAddParam}>
        {pending ? (
          <>
            <DialogHeader>Add device</DialogHeader>
            <p class="invite-desc">Add this device to your account.</p>
            <div class="device-code-summary">
              <span class="device-code-summary-name">{pending.name}</span>
              <Badge>{pending.platform}</Badge>
            </div>
            {error && <p class="device-code-error">{error}</p>}
            <DialogActions>
              <Button variant="ghost" type="button" onClick={reset}>
                Back
              </Button>
              <Button variant="primary" type="button" onClick={handleApprove} disabled={loading}>
                {loading ? 'Adding…' : 'Add'}
              </Button>
            </DialogActions>
          </>
        ) : (
          <>
            <DialogHeader>Enter device code</DialogHeader>
            <form onSubmit={handleLookup}>
              <Input
                class="device-code-input"
                value={code}
                onInput={(e: InputEvent) =>
                  handleCodeInput(
                    e.target as HTMLInputElement,
                    e.inputType === 'deleteContentBackward',
                  )
                }
                placeholder="XXX-XXX"
                aria-label="Device code"
                autocomplete="off"
                autoCorrect="off"
                spellcheck={false}
                required
                autoFocus
              />
              {error && <p class="device-code-error">{error}</p>}
              <p class="invite-desc">
                Open Virtue on the device you want to monitor. It shows the code to type here.
              </p>
              <DialogActions
                left={
                  <Button variant="ghost" href={DOWNLOAD_URL} target="_blank" rel="noreferrer">
                    Download Virtue Initiative
                  </Button>
                }
              >
                <Button variant="primary" type="submit" disabled={loading || userCode.length !== 6}>
                  {loading ? 'Checking…' : 'Continue'}
                </Button>
              </DialogActions>
            </form>
          </>
        )}
      </Dialog>
    </>
  );
}

function DeviceCard({
  device,
  onUpdateDevice,
  onRemoveDevice,
}: {
  device: Device;
  onUpdateDevice: (id: string, patch: { name?: string }) => Promise<void>;
  onRemoveDevice: (id: string) => Promise<void>;
}) {
  const { route } = useLocation();
  const [name, setName] = useState(device.name);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const { push: pushToast } = useToast();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  function openEdit() {
    setName(device.name);
    dialogRef.current?.showModal();
  }

  function closeEdit() {
    dialogRef.current?.close();
  }

  function openDeleteDialog() {
    dialogRef.current?.close();
    deleteDialogRef.current?.showModal();
  }

  function closeDeleteDialog() {
    deleteDialogRef.current?.close();
  }

  async function handleSave(e: Event) {
    e.preventDefault();
    setSaving(true);
    try {
      await onUpdateDevice(device.id, { name });
      dialogRef.current?.close();
    } catch (err) {
      const message = describeError(err, 'Failed to save');
      if (message) pushToast(message, 'error');
    } finally {
      setSaving(false);
    }
  }

  async function handleDeleteConfirmed() {
    setDeleting(true);
    try {
      await onRemoveDevice(device.id);
      closeDeleteDialog();
    } catch (err) {
      const message = describeError(err, 'Failed to delete device');
      if (message) pushToast(message, 'error');
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <span class="vi-card__name">{device.name}</span>
        <DeviceStatusBadge status={device.status} />
      </CardHeader>
      <dl class="vi-card__meta">
        <dt>Platform</dt>
        <dd>{device.platform}</dd>
        <dt>Last upload</dt>
        <dd>{formatRelativeTimestamp(device.last_upload_at)}</dd>
        <dt>Last activity</dt>
        <dd>{formatRelativeTimestamp(device.last_hash_at)}</dd>
      </dl>
      <CardActions>
        <Button variant="ghost" type="button" onClick={() => route(`/logs?device_id=${device.id}`)}>
          View logs
        </Button>
        <Button variant="ghost" type="button" onClick={openEdit}>
          Edit
        </Button>
      </CardActions>

      <Dialog dialogRef={dialogRef}>
        <DialogHeader>Edit device</DialogHeader>
        <form onSubmit={handleSave}>
          <Field label="Name">
            <Input
              type="text"
              value={name}
              onInput={(e) => setName((e.target as HTMLInputElement).value)}
              required
            />
          </Field>
          <DialogSecondaryActions>
            <Button
              variant="danger"
              type="button"
              onClick={openDeleteDialog}
              disabled={saving || deleting}
            >
              Delete device permanently
            </Button>
          </DialogSecondaryActions>
          <DialogActions>
            <Button variant="ghost" type="button" onClick={closeEdit} disabled={saving || deleting}>
              Cancel
            </Button>
            <Button variant="primary" type="submit" disabled={saving || deleting}>
              {saving ? 'Saving…' : 'Save'}
            </Button>
          </DialogActions>
        </form>
      </Dialog>

      <Dialog dialogRef={deleteDialogRef}>
        <DialogHeader>Delete device</DialogHeader>
        <p class="invite-desc">
          Delete "{device.name}"? This permanently removes its logs and uploads, and your partners
          will be notified.
        </p>
        <DialogActions>
          <Button
            variant="ghost"
            type="button"
            onClick={closeDeleteDialog}
            disabled={saving || deleting}
          >
            Cancel
          </Button>
          <Button
            variant="danger"
            type="button"
            onClick={() => handleDeleteConfirmed().catch(() => {})}
            disabled={saving || deleting}
          >
            {deleting ? 'Deleting…' : 'Delete device'}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
