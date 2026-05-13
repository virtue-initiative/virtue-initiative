import { useMemo, useRef, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { Device, WatchingPartner, WatcherPartner } from "../../api";
import {
  Alert,
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
} from "@virtueinitiative/shared-web";
import { useAuth } from "../../context/auth";
import { useDevices } from "../../hooks/useDevices";
import { usePartners } from "../../hooks/usePartners";
import { formatRelativeTimestamp } from "../../utils/time";
import "./style.css";

const DOWNLOAD_URL = "https://virtueinitiative.org/download";
const INSTALLATION_URL = "https://virtueinitiative.org/help/installation/";

function UserPlusIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={1.5}
      width="1.1em"
      height="1.1em"
      style={{ flexShrink: 0 }}
    >
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M18 7.5v3m0 0v3m0-3h3m-3 0h-3m-2.25-4.125a3.375 3.375 0 1 1-6.75 0 3.375 3.375 0 0 1 6.75 0ZM3 19.235v-.11a6.375 6.375 0 0 1 12.75 0v.109A12.318 12.318 0 0 1 9.374 21c-2.331 0-4.512-.645-6.374-1.766Z"
      />
    </svg>
  );
}

export function Home() {
  const { userId } = useAuth();
  const {
    devices,
    error: devicesError,
    isLoading: devicesLoading,
    updateDevice,
    removeDevice,
  } = useDevices();
  const {
    watching,
    watchers,
    error: partnersError,
    isLoading: partnersLoading,
    invitePartner,
    removeWatching,
    removeWatcher,
  } = usePartners();
  const error = devicesError ?? partnersError;
  const dashboardLoading = devicesLoading || partnersLoading;
  const deviceList = devices ?? [];
  const watchingList = watching ?? [];
  const watchersList = watchers ?? [];

  const ownDevices = useMemo(
    () => deviceList.filter((device) => device.owner === userId),
    [deviceList, userId],
  );
  const devicesByOwner = useMemo(() => {
    const map = new Map<string, Device[]>();
    for (const device of deviceList) {
      const ownerDevices = map.get(device.owner) ?? [];
      ownerDevices.push(device);
      map.set(device.owner, ownerDevices);
    }
    return map;
  }, [deviceList]);
  const acceptedWatching = useMemo(
    () => watchingList.filter((partner) => partner.status === "accepted"),
    [watchingList],
  );
  const pendingWatching = useMemo(
    () => watchingList.filter((partner) => partner.status === "pending"),
    [watchingList],
  );
  const acceptedWatchers = useMemo(
    () => watchersList.filter((partner) => partner.status === "accepted"),
    [watchersList],
  );
  const pendingWatchers = useMemo(
    () => watchersList.filter((partner) => partner.status === "pending"),
    [watchersList],
  );

  return (
    <div class="dashboard">
      {error && <Alert variant="error">{error.message}</Alert>}
      {dashboardLoading && !devices && !watching && !watchers && (
        <p class="empty">Loading…</p>
      )}

      {!dashboardLoading && (
        <>
          <section class="dashboard-section">
            <div class="dashboard-section-header">
              <h2>My devices</h2>
              <AddDeviceButton />
            </div>
            {ownDevices.length === 0 ? (
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
          </section>

          <section class="dashboard-section">
            <div class="dashboard-section-header">
              <h2>You monitor</h2>
            </div>
            <PartnerArea
              emptyLabel="You cannot monitor anyone yet."
              pending={pendingWatching}
              accepted={acceptedWatching}
              partnerDevicesByOwner={devicesByOwner}
              onRemoveWatching={removeWatching}
              onRemoveWatcher={removeWatcher}
            />
          </section>

          <section class="dashboard-section">
            <div class="dashboard-section-header">
              <h2>Monitor you</h2>
              <InviteButton onInvitePartner={invitePartner} />
            </div>
            <PartnerArea
              emptyLabel="No one can monitor you yet."
              pending={pendingWatchers}
              accepted={acceptedWatchers}
              partnerDevicesByOwner={devicesByOwner}
              onRemoveWatching={removeWatching}
              onRemoveWatcher={removeWatcher}
            />
          </section>
        </>
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
          Set up Virtue on a phone or computer, then sign in with this account
          so it starts appearing in your dashboard.
        </p>
        <ol class="device-setup-steps">
          <li>
            <span class="device-setup-step-label">Download the app.</span>
            Choose the installer for the device you want to monitor.
          </li>
          <li>
            <span class="device-setup-step-label">
              Follow the installation instructions.
            </span>
            Use the platform-specific setup guide if you need it.
          </li>
          <li>
            <span class="device-setup-step-label">Log in on that device.</span>
            Once the app signs in and uploads, it will show up here.
          </li>
        </ol>
        <DialogActions
          left={
            <Button
              variant="ghost"
              href={INSTALLATION_URL}
              target="_blank"
              rel="noreferrer"
            >
              View guide
            </Button>
          }
        >
          <Button variant="ghost" type="button" onClick={close}>
            Close
          </Button>
          <Button
            variant="primary"
            href={DOWNLOAD_URL}
            target="_blank"
            rel="noreferrer"
          >
            Download
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}

function PartnerArea({
  emptyLabel,
  pending,
  accepted,
  partnerDevicesByOwner,
  onRemoveWatching,
  onRemoveWatcher,
}: {
  emptyLabel: string;
  pending: Array<WatchingPartner | WatcherPartner>;
  accepted: Array<WatchingPartner | WatcherPartner>;
  partnerDevicesByOwner: Map<string, Device[]>;
  onRemoveWatching: (id: string) => Promise<void>;
  onRemoveWatcher: (id: string) => Promise<void>;
}) {
  const partners = [...pending, ...accepted];

  return (
    <section class="partners-panel">
      {partners.length === 0 ? (
        <p class="empty">{emptyLabel}</p>
      ) : (
        <CardGrid>
          {partners.map((partner) =>
            partner.status === "pending" ? (
              <PendingPartnerCard
                key={partner.id}
                partner={partner}
                onRemoveWatching={onRemoveWatching}
                onRemoveWatcher={onRemoveWatcher}
              />
            ) : (
              <PartnerCard
                key={partner.id}
                partner={partner}
                devices={
                  "digest_cadence" in partner
                    ? (partnerDevicesByOwner.get(partner.user.id) ?? [])
                    : []
                }
                onRemoveWatching={onRemoveWatching}
                onRemoveWatcher={onRemoveWatcher}
              />
            ),
          )}
        </CardGrid>
      )}
    </section>
  );
}

function InviteButton({
  onInvitePartner,
}: {
  onInvitePartner: (email: string) => Promise<void>;
}) {
  const [email, setEmail] = useState("");
  const [loading, setLoading] = useState(false);
  const { push: pushToast } = useToast();
  const dialogRef = useRef<HTMLDialogElement>(null);

  function open() {
    setEmail("");
    dialogRef.current?.showModal();
  }

  function close() {
    dialogRef.current?.close();
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setLoading(true);
    try {
      await onInvitePartner(email);
      close();
    } catch (err) {
      pushToast(
        err instanceof Error ? err.message : "Failed to send invite",
        "error",
      );
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <Button variant="primary" type="button" onClick={open} style={{ gap: "0.4rem" }}>
        <UserPlusIcon /> Invite partner
      </Button>
      <Dialog dialogRef={dialogRef}>
        <DialogHeader>Invite a partner</DialogHeader>
        <p class="invite-desc">
          Your partner can <b>view any screenshots and activity logs </b>
          uploaded <b>after</b> you add them as a partner and they set up their
          account.
        </p>
        <form onSubmit={handleSubmit}>
          <Field label="Partner's email">
            <Input
              type="email"
              value={email}
              onInput={(e) => setEmail((e.target as HTMLInputElement).value)}
              placeholder="partner@example.com"
              required
              autoFocus
            />
          </Field>
          <DialogActions>
            <Button variant="ghost" type="button" onClick={close}>
              Cancel
            </Button>
            <Button variant="primary" type="submit" disabled={loading}>
              {loading ? "Sending…" : "Send invite"}
            </Button>
          </DialogActions>
        </form>
      </Dialog>
    </>
  );
}

function PendingPartnerCard({
  partner,
  onRemoveWatching,
  onRemoveWatcher,
}: {
  partner: WatchingPartner | WatcherPartner;
  onRemoveWatching: (id: string) => Promise<void>;
  onRemoveWatcher: (id: string) => Promise<void>;
}) {
  const [action, setAction] = useState<"remove" | null>(null);
  const { push: pushToast } = useToast();
  const confirmRef = useRef<HTMLDialogElement>(null);
  const partnerLabel = partner.user.name ?? partner.user.email;
  const partnerEmailTooltip = partner.user.name
    ? undefined
    : partner.user.email;
  const partnerName = partnerLabel;

  async function removeConfirmed() {
    setAction("remove");
    try {
      await ("digest_cadence" in partner
        ? onRemoveWatching(partner.id)
        : onRemoveWatcher(partner.id));
    } catch (err) {
      pushToast(
        err instanceof Error ? err.message : "Failed to remove request",
        "error",
      );
      setAction(null);
    }
  }

  return (
    <Card>
      <CardHeader>
        <span class="vi-card__name" title={partnerEmailTooltip}>
          {partnerLabel}
        </span>
        <Badge variant="yellow">Pending</Badge>
      </CardHeader>
      <CardActions>
        <Button
          variant="danger"
          type="button"
          onClick={() => confirmRef.current?.showModal()}
          disabled={action !== null}
        >
          {action === "remove" ? "Removing…" : "Remove"}
        </Button>
      </CardActions>
      <Dialog dialogRef={confirmRef}>
        <DialogHeader>Remove {partnerName}?</DialogHeader>
        <p class="invite-desc">
          This will cancel the pending partner relationship.
        </p>
        <DialogActions>
          <Button
            variant="ghost"
            type="button"
            onClick={() => confirmRef.current?.close()}
            disabled={action !== null}
          >
            Cancel
          </Button>
          <Button
            variant="danger"
            type="button"
            onClick={() => {
              confirmRef.current?.close();
              removeConfirmed().catch(() => {});
            }}
            disabled={action !== null}
          >
            {action === "remove" ? "Removing…" : "Remove partner"}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}

function PartnerCard({
  partner,
  devices,
  onRemoveWatching,
  onRemoveWatcher,
}: {
  partner: WatchingPartner | WatcherPartner;
  devices: Device[];
  onRemoveWatching: (id: string) => Promise<void>;
  onRemoveWatcher: (id: string) => Promise<void>;
}) {
  const { route } = useLocation();
  const isWatching = "digest_cadence" in partner;
  const [action, setAction] = useState<"remove" | null>(null);
  const { push: pushToast } = useToast();
  const confirmRef = useRef<HTMLDialogElement>(null);
  const partnerLabel = partner.user.name ?? partner.user.email;
  const partnerEmailTooltip = partner.user.name
    ? undefined
    : partner.user.email;
  const partnerName = partnerLabel;

  async function removeConfirmed() {
    setAction("remove");
    try {
      await ("digest_cadence" in partner
        ? onRemoveWatching(partner.id)
        : onRemoveWatcher(partner.id));
    } catch (err) {
      pushToast(
        err instanceof Error ? err.message : "Failed to remove partner",
        "error",
      );
      setAction(null);
    }
  }

  return (
    <Card class={isWatching ? undefined : "partner-card-compact"}>
      <CardHeader>
        <span class="vi-card__name" title={partnerEmailTooltip}>
          {partnerLabel}
        </span>
      </CardHeader>
      {isWatching && devices.length > 0 && (
        <div class="partner-device-list">
          {devices.slice(0, 4).map((device) => (
            <button
              key={device.id}
              class="partner-device-chip"
              type="button"
              onClick={() =>
                route(`/logs?user=${partner.user.id}&device_id=${device.id}`)
              }
              title={device.name}
            >
              <span
                class={`partner-device-status ${device.status === "online" ? "partner-device-status-online" : "partner-device-status-offline"}`}
              />
              <span>{device.name}</span>
            </button>
          ))}
          {devices.length > 4 && (
            <p class="partner-device-more">
              +{devices.length - 4} more devices
            </p>
          )}
        </div>
      )}
      <CardActions>
        {isWatching && partner.user.id && (
          <Button
            variant="ghost"
            type="button"
            onClick={() => route(`/logs?user=${partner.user.id}`)}
          >
            View logs
          </Button>
        )}
        <Button
          variant="danger"
          type="button"
          onClick={() => confirmRef.current?.showModal()}
          disabled={action !== null}
        >
          {action === "remove" ? "Removing…" : "Remove"}
        </Button>
      </CardActions>
      <Dialog dialogRef={confirmRef}>
        <DialogHeader>Remove {partnerName}?</DialogHeader>
        <p class="invite-desc">
          This will remove your partner relationship with this person. The
          partner will be notified.
        </p>
        <DialogActions>
          <Button
            variant="ghost"
            type="button"
            onClick={() => confirmRef.current?.close()}
            disabled={action !== null}
          >
            Cancel
          </Button>
          <Button
            variant="danger"
            type="button"
            onClick={() => {
              confirmRef.current?.close();
              removeConfirmed().catch(() => {});
            }}
            disabled={action !== null}
          >
            {action === "remove" ? "Removing…" : "Remove partner"}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
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
      pushToast(
        err instanceof Error ? err.message : "Failed to save",
        "error",
      );
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
      pushToast(
        err instanceof Error ? err.message : "Failed to delete device",
        "error",
      );
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <span class="vi-card__name">{device.name}</span>
        <Badge variant={device.status === "online" ? "green" : "gray"}>
          {device.status === "online" ? "Online" : "Offline"}
        </Badge>
      </CardHeader>
      <dl class="vi-card__meta">
        <dt>Platform</dt>
        <dd>{device.platform}</dd>
        <dt>Last upload</dt>
        <dd>{formatRelativeTimestamp(device.last_upload_at)}</dd>
      </dl>
      <CardActions>
        <Button
          variant="ghost"
          type="button"
          onClick={() => route(`/logs?device_id=${device.id}`)}
        >
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
            <Button
              variant="ghost"
              type="button"
              onClick={closeEdit}
              disabled={saving || deleting}
            >
              Cancel
            </Button>
            <Button
              variant="primary"
              type="submit"
              disabled={saving || deleting}
            >
              {saving ? "Saving…" : "Save"}
            </Button>
          </DialogActions>
        </form>
      </Dialog>

      <Dialog dialogRef={deleteDialogRef}>
        <DialogHeader>Delete device</DialogHeader>
        <p class="invite-desc">
          Delete "{device.name}"? This permanently removes its logs and uploads,
          and your partners will be notified.
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
            {deleting ? "Deleting…" : "Delete device"}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
