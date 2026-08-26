import { useMemo, useRef, useState } from 'preact/hooks';
import { useLocation } from 'preact-iso';
import { Device, describeError, useAPIContext, useDevices } from '../../utils/api';
import { PageHeading } from '../../components/PageHeading';
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

function AddDeviceButton() {
  const dialogRef = useRef<HTMLDialogElement>(null);

  function open() {
    dialogRef.current?.showModal();
  }

  function close() {
    dialogRef.current?.close();
  }

  return (
    <>
      <Button variant="primary" type="button" onClick={open}>
        Add device
      </Button>
      <Dialog dialogRef={dialogRef} class="device-setup-dialog">
        <DialogHeader>Add device</DialogHeader>
        <p class="invite-desc">
          Set up Virtue on a phone or computer, then sign in with this account so it starts
          appearing in your dashboard.
        </p>
        <ol class="device-setup-steps">
          <li>
            <span class="device-setup-step-label">Download the app.</span>
            Choose the installer for the device you want to monitor.
          </li>
          <li>
            <span class="device-setup-step-label">Follow the installation instructions.</span>
            Use the platform-specific setup guide if you need it.
          </li>
          <li>
            <span class="device-setup-step-label">Log in on that device.</span>
            Once the app signs in and uploads, it will show up here.
          </li>
        </ol>
        <DialogActions
          left={
            <Button variant="ghost" href={DOWNLOAD_URL} target="_blank" rel="noreferrer">
              View guide
            </Button>
          }
        >
          <Button variant="ghost" type="button" onClick={close}>
            Close
          </Button>
          <Button variant="primary" href={DOWNLOAD_URL} target="_blank" rel="noreferrer">
            Download
          </Button>
        </DialogActions>
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
        <Badge variant={device.status === 'online' ? 'green' : 'gray'}>
          {device.status === 'online'
            ? 'Online'
            : device.status === 'logged_out'
              ? 'Logged out'
              : 'Offline'}
        </Badge>
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
              Delete device
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
