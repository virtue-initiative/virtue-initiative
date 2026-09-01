import { useMemo, useRef, useState } from 'preact/hooks';
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
  const [firstHalf, setFirstHalf] = useState('');
  const [secondHalf, setSecondHalf] = useState('');
  const [pending, setPending] = useState<DeviceCodeLookupResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // One ref on the wrapping element rather than two on the `Input` components:
  // `Input` is a plain function component, so a ref passed to it would not reach
  // the underlying `<input>`.
  const codeInputsRef = useRef<HTMLDivElement>(null);
  const { push: pushToast } = useToast();

  const userCode = `${firstHalf}${secondHalf}`;

  function reset() {
    setFirstHalf('');
    setSecondHalf('');
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
   * Split whatever landed in a box across the two boxes, so pasting or typing a
   * whole `K7R-M3X` into the first one fills both instead of being truncated.
   * Characters outside the code alphabet (the separator included) are dropped;
   * the server normalizes the same way (API-046).
   */
  function handleCodeInput(box: 'first' | 'second', raw: string) {
    const cleaned = raw.toUpperCase().replace(/[^0-9A-Z]/g, '');

    if (box === 'first') {
      setFirstHalf(cleaned.slice(0, 3));
      if (cleaned.length > 3) {
        setSecondHalf(cleaned.slice(3, 6));
      }
      if (cleaned.length >= 3) {
        focusSecondBox();
      }
      return;
    }

    setSecondHalf(cleaned.slice(0, 3));
  }

  function focusSecondBox() {
    codeInputsRef.current?.querySelectorAll('input')[1]?.focus();
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
        <DialogHeader>Add device</DialogHeader>
        {pending ? (
          <>
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
            <p class="invite-desc">
              Open Virtue on the device you want to monitor and sign in. It shows a code. Type that
              code here.
            </p>
            <form onSubmit={handleLookup}>
              <Field label="Device code">
                <div class="device-code-inputs" ref={codeInputsRef}>
                  <Input
                    class="device-code-input"
                    value={firstHalf}
                    onInput={(e) => handleCodeInput('first', (e.target as HTMLInputElement).value)}
                    aria-label="Device code, first three characters"
                    autocomplete="off"
                    autoCorrect="off"
                    spellcheck={false}
                    required
                    autoFocus
                  />
                  <span class="device-code-separator" aria-hidden="true">
                    –
                  </span>
                  <Input
                    class="device-code-input"
                    value={secondHalf}
                    onInput={(e) => handleCodeInput('second', (e.target as HTMLInputElement).value)}
                    aria-label="Device code, last three characters"
                    autocomplete="off"
                    autoCorrect="off"
                    spellcheck={false}
                    required
                  />
                </div>
              </Field>
              {error && <p class="device-code-error">{error}</p>}
              <DialogActions
                left={
                  <Button variant="ghost" href={DOWNLOAD_URL} target="_blank" rel="noreferrer">
                    Download
                  </Button>
                }
              >
                <Button variant="ghost" href={DOWNLOAD_URL} target="_blank" rel="noreferrer">
                  View guide
                </Button>
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
