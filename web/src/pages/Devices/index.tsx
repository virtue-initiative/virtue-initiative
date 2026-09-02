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
  // The clients print a `/devices?add` link, so following it should land on the
  // code box rather than on a page with a button still to find.
  const { url } = useLocation();
  const autoOpened = useRef(false);

  useEffect(() => {
    const query = url.includes('?') ? url.slice(url.indexOf('?')) : '';
    if (!new URLSearchParams(query).has('add') || autoOpened.current) return;
    autoOpened.current = true;
    open();
  }, [url]);

  const userCode = code.replace('-', '');

  function reset() {
    setCode('');
    setPending(null);
    setError(null);
    setLoading(false);
  }

  function open() {
    reset();
    dialogRef.current?.showModal();
  }

  function close() {
    dialogRef.current?.close();
  }

  /**
   * Keeps the box showing `XXX-XXX` however the code arrives: typed without the
   * dash, pasted with one, lowercase, or spaced. Characters outside the code
   * alphabet are dropped rather than rejected, which is how the server
   * normalizes too (API-046), so a pasted `k7r m3x` just works.
   */
  function handleCodeInput(raw: string) {
    const cleaned = raw
      .toUpperCase()
      .replace(/[^0-9A-Z]/g, '')
      .slice(0, 6);
    setCode(cleaned.length > 3 ? `${cleaned.slice(0, 3)}-${cleaned.slice(3)}` : cleaned);
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
      <Button variant="primary" type="button" onClick={open}>
        Add device
      </Button>
      <Dialog dialogRef={dialogRef} class="device-setup-dialog">
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
                onInput={(e) => handleCodeInput((e.target as HTMLInputElement).value)}
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
                Open Virtue on the device you want to monitor and sign in. It shows the code to type
                here.
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
