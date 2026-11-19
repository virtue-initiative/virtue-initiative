import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { useSingleFlight } from '../../hooks/usePromise';
import { useLocation } from 'preact-iso';
import { Device, describeError, useAPIContext, useDevices, useUser } from '../../utils/api';
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
  Switch,
  useToast,
} from '@virtueinitiative/shared-web';
import { formatRelativeTimestamp } from '../../utils/time';
import { LANDING_URL } from '../../utils/landing-url';
import './style.css';

type DevicePatch = { name?: string; dns_filter_disabled?: boolean };

const DOWNLOAD_URL = `${LANDING_URL}/download`;

export function Devices() {
  const api = useAPIContext();
  const userId = api?.userId ?? null;
  const { devices, loaded } = useDevices();
  const accountDomain = useUser()?.dns_filter_domain ?? null;

  // The device cache otherwise lives for the whole session, so status and
  // last-upload times would go stale. Refetch on open; cached cards stay up.
  useEffect(() => {
    void api?.refreshDevices();
  }, [api]);

  const updateDevice = (id: string, patch: DevicePatch) =>
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
              accountDomain={accountDomain}
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
// latest comes from there, after pulling in any new batch rows. Null while unknown: until
// it answers, when the cache has never synced and can't now, or if this browser has no
// cache. Cards then show "—" rather than claiming a device never uploaded.
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
  accountDomain,
  lastUpload,
  onUpdateDevice,
  onRemoveDevice,
}: {
  device: Device;
  // The account's DNS check domain (WEB-073), or null when none is set.
  accountDomain: string | null;
  // undefined while unknown, null when the device has never uploaded.
  lastUpload: number | null | undefined;
  onUpdateDevice: (id: string, patch: DevicePatch) => Promise<void>;
  onRemoveDevice: (id: string) => Promise<void>;
}) {
  const { route } = useLocation();
  const [name, setName] = useState(device.name);
  const [dnsCheck, setDnsCheck] = useState(!device.dns_filter_disabled);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const once = useSingleFlight();
  const { push: pushToast } = useToast();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);
  const dnsOffDialogRef = useRef<HTMLDialogElement>(null);
  const dnsOffConfirmed = useRef(false);

  function openEdit() {
    setName(device.name);
    setDnsCheck(!device.dns_filter_disabled);
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

  // WEB-072: turning the check off is confirmed on save, in a second dialog over this one.
  function handleSave(e: Event) {
    e.preventDefault();
    if (!dnsCheck && !device.dns_filter_disabled) {
      dnsOffConfirmed.current = false;
      dnsOffDialogRef.current?.showModal();
      return;
    }
    return save();
  }

  function confirmDnsOff() {
    dnsOffConfirmed.current = true;
    dnsOffDialogRef.current?.close();
    return save();
  }

  // Closing the confirmation any other way (button, Escape, backdrop) keeps the check on.
  function handleDnsOffClosed() {
    if (!dnsOffConfirmed.current) setDnsCheck(true);
  }

  async function save() {
    await once(async () => {
      setSaving(true);
      try {
        const patch: DevicePatch = { name };
        // WEB-072: only send the switch when it changed.
        if (dnsCheck === !!device.dns_filter_disabled) patch.dns_filter_disabled = !dnsCheck;
        await onUpdateDevice(device.id, patch);
        dialogRef.current?.close();
      } catch (err) {
        const message = describeError(err, 'Failed to save');
        if (message) pushToast(message, 'error');
      } finally {
        setSaving(false);
      }
    });
  }

  async function handleDeleteConfirmed() {
    await once(async () => {
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
    });
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
        <dt>DNS check</dt>
        <dd>{device.dns_filter_disabled ? 'Off' : accountDomain ? 'On' : 'Not set up'}</dd>
      </dl>
      <CardActions>
        <Button variant="ghost" type="button" onClick={() => route(`/logs?device_id=${device.id}`)}>
          View logs
        </Button>
        <Button variant="ghost" type="button" onClick={openEdit}>
          Edit
        </Button>
      </CardActions>

      <Dialog dialogRef={dialogRef} busy={saving}>
        <DialogHeader>Edit device</DialogHeader>
        <form onSubmit={handleSave}>
          <Field label="Name" id={`device-name-${device.id}`}>
            <Input
              id={`device-name-${device.id}`}
              type="text"
              value={name}
              onInput={(e) => setName((e.target as HTMLInputElement).value)}
              required
            />
          </Field>
          <div class="device-dns-check">
            <Switch
              id={`device-dns-${device.id}`}
              label="Check the DNS filter on this device"
              checked={dnsCheck}
              onChange={setDnsCheck}
            />
            {accountDomain ? (
              <p class="hint-text">
                This device checks that your DNS check domain, {accountDomain}, stays blocked. Turn
                it off for a device that uses its own DNS, such as a work laptop. Your partners are
                notified when you turn it off.
              </p>
            ) : (
              <p class="hint-text">
                Set a DNS check domain in <a href="/settings">Settings</a> to use this check.
              </p>
            )}
          </div>
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

      <Dialog dialogRef={dnsOffDialogRef} onClose={handleDnsOffClosed}>
        <DialogHeader>Turn off the DNS check on this device?</DialogHeader>
        <p class="invite-desc">
          This check is here to protect you. If you turn it off for this device, your partners are
          notified.
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
          <Button variant="danger" type="button" onClick={() => void confirmDnsOff()}>
            Turn off
          </Button>
        </DialogActions>
      </Dialog>

      <Dialog dialogRef={deleteDialogRef} busy={deleting}>
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
