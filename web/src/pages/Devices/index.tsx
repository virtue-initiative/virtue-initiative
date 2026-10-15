import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { useLocation } from 'preact-iso';
import { Device, describeError, useAPIContext, useDevices } from '../../utils/api';
import { cacheClient } from '../../utils/cache';
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

  // The device cache otherwise lives for the whole session, so status and
  // last-upload times would go stale. Refetch on open; cached cards stay up.
  useEffect(() => {
    void api?.refreshDevices();
  }, [api]);

  const updateDevice = (id: string, patch: { name?: string }) =>
    api ? api.updateDevice(id, patch) : Promise.reject(new Error('Not signed in'));
  const removeDevice = (id: string) =>
    api ? api.removeDevice(id) : Promise.reject(new Error('Not signed in'));

  const ownDevices = useMemo(
    () => devices.filter((device) => device.owner === userId),
    [devices, userId],
  );
  const lastUploads = useLastUploads(userId, devices);

  return (
    <div class="dashboard">
      <PageHeading icon={<DevicesIcon />} actions={<AddDeviceButton />}>
        Devices
      </PageHeading>
      {!loaded ? (
        <p class="loading">Loading…</p>
      ) : ownDevices.length === 0 ? (
        <EmptyDevices />
      ) : (
        <CardGrid>
          {ownDevices.map((device) => (
            <DeviceCard
              key={device.id}
              device={device}
              lastUpload={lastUploads ? (lastUploads[device.id] ?? null) : undefined}
              onUpdateDevice={updateDevice}
              onRemoveDevice={removeDevice}
            />
          ))}
        </CardGrid>
      )}
    </div>
  );
}

function DeviceSetupSteps() {
  return (
    <>
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
          Once the app signs in and uploads, it will show up here. On iPhone and iPad, also turn on
          the Safari extension.
        </li>
      </ol>
      <p class="device-setup-note">
        The iPhone and iPad app is in TestFlight, not the App Store. Get it from the download page.
      </p>
    </>
  );
}

function EmptyDevices() {
  return (
    <section class="devices-empty" aria-labelledby="devices-empty-title">
      <span class="devices-empty-icon">
        <DevicesIcon />
      </span>
      <h2 id="devices-empty-title" class="devices-empty-title">
        Add your first device
      </h2>
      <p class="devices-empty-desc">
        Nothing is being monitored yet. Install Virtue on a phone or computer and sign in with this
        account to get started.
      </p>
      <DeviceSetupSteps />
      <div class="devices-empty-actions">
        <Button variant="ghost" href={DOWNLOAD_URL} target="_blank" rel="noreferrer">
          View guide
        </Button>
        <Button variant="primary" href={DOWNLOAD_URL} target="_blank" rel="noreferrer">
          Download the app
        </Button>
      </div>
    </section>
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
        <DeviceSetupSteps />
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

// GET /device doesn't report last uploads (API-029), since the server would have to read
// every batch to find them. The cache already holds this user's batches, so each device's
// latest comes from there, after pulling in any new batch rows. Null until it answers, or
// for good if this browser has no cache.
function useLastUploads(userId: string | null, devices: Device[]) {
  const [lastUploads, setLastUploads] = useState<Record<string, number> | null>(null);

  useEffect(() => {
    if (!userId || !cacheClient) return;
    let cancelled = false;
    cacheClient
      .getDeviceLastUploads(userId, userId)
      .then((result) => {
        if (!cancelled) setLastUploads(result);
      })
      .catch((err) => console.warn('[devices] failed to read last uploads', err));
    return () => {
      cancelled = true;
    };
  }, [userId, devices]);

  return lastUploads;
}

function DeviceCard({
  device,
  lastUpload,
  onUpdateDevice,
  onRemoveDevice,
}: {
  device: Device;
  // undefined while unknown, null when the device has never uploaded.
  lastUpload: number | null | undefined;
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
        <dd>{lastUpload === undefined ? '—' : formatRelativeTimestamp(lastUpload)}</dd>
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
