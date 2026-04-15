import { useMemo, useRef, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { Device, WatchingPartner, WatcherPartner } from "../../api";
import { Button } from "../../components/Button";
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
      style="stroke-width: 1.5"
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
      {error && <p class="alert-error">{error.message}</p>}
      {dashboardLoading && !devices && !watching && !watchers && (
        <p class="empty">Loading…</p>
      )}

      {!dashboardLoading && (
        <>
          <section class="dash-section">
            <div class="section-header">
              <h2>My devices</h2>
              <AddDeviceButton />
            </div>
            {ownDevices.length === 0 ? (
              <p class="empty">No devices</p>
            ) : (
              <div class="card-grid">
                {ownDevices.map((device) => (
                  <DeviceCard
                    key={device.id}
                    device={device}
                    onUpdateDevice={updateDevice}
                    onRemoveDevice={removeDevice}
                  />
                ))}
              </div>
            )}
          </section>

          <section class="dash-section">
            <div class="section-header">
              <h2>People you can monitor</h2>
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

          <section class="dash-section">
            <div class="section-header">
              <h2>People who can monitor you</h2>
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

  function handleDialogClick(e: MouseEvent) {
    if (e.target === dialogRef.current) {
      close();
    }
  }

  return (
    <>
      <Button className="btn-primary" onClick={open}>
        Add device
      </Button>
      <dialog
        ref={dialogRef}
        class="device-setup-dialog"
        onClick={handleDialogClick}
      >
        <h3 class="dialog-title">Add device</h3>
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
        <div class="invite-actions device-setup-actions">
          <a
            class="btn btn-primary"
            href={DOWNLOAD_URL}
            target="_blank"
            rel="noreferrer"
          >
            Download
          </a>
          <a
            class="btn btn-ghost"
            href={INSTALLATION_URL}
            target="_blank"
            rel="noreferrer"
          >
            Guide
          </a>
          <button
            class="btn btn-ghost device-setup-close"
            type="button"
            onClick={close}
          >
            Close
          </button>
        </div>
      </dialog>
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
        <div class="card-grid">
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
        </div>
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
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);

  function open() {
    setEmail("");
    setError(null);
    dialogRef.current?.showModal();
  }

  function close() {
    dialogRef.current?.close();
    setError(null);
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      await onInvitePartner(email);
      close();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to send invite");
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <Button className="btn-primary" onClick={open} icon={<UserPlusIcon />}>
        Invite partner
      </Button>
      <dialog ref={dialogRef}>
        <h3 class="dialog-title">Invite a partner</h3>
        <p class="invite-desc">
          Your partner can <b>view any screenshots and activity logs </b>
          uploaded <b>after</b> you add them as a partner and they set up their
          account.
        </p>
        <form onSubmit={handleSubmit}>
          <div class="field">
            <label for="invite-email">Partner's email</label>
            <input
              id="invite-email"
              type="email"
              value={email}
              onInput={(e) => setEmail((e.target as HTMLInputElement).value)}
              placeholder="partner@example.com"
              required
              autoFocus
            />
          </div>
          {error && <p class="alert-error">{error}</p>}
          <div class="invite-actions">
            <button class="btn btn-primary" type="submit" disabled={loading}>
              {loading ? "Sending…" : "Send invite"}
            </button>
            <button class="btn btn-ghost" type="button" onClick={close}>
              Cancel
            </button>
          </div>
        </form>
      </dialog>
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
  const [error, setError] = useState<string | null>(null);
  const confirmRef = useRef<HTMLDialogElement>(null);
  const partnerName = partner.user.name ?? partner.user.email;

  async function removeConfirmed() {
    setAction("remove");
    setError(null);
    try {
      await ("digest_cadence" in partner
        ? onRemoveWatching(partner.id)
        : onRemoveWatcher(partner.id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to remove request");
      setAction(null);
    }
  }

  return (
    <div class="card card-highlight">
      <div class="card-header">
        <span class="card-name">{partner.user.name ?? partner.user.email}</span>
        <span class="badge badge-yellow">Pending</span>
      </div>
      {error && <p class="alert-error">{error}</p>}
      <div class="card-actions">
        <button
          class="btn btn-danger"
          type="button"
          onClick={() => confirmRef.current?.showModal()}
          disabled={action !== null}
        >
          {action === "remove" ? "Removing…" : "Remove"}
        </button>
      </div>
      <dialog ref={confirmRef}>
        <h3 class="dialog-title">Remove {partnerName}?</h3>
        <p class="invite-desc">
          This will cancel the pending partner relationship. The partner will be
          notified.
        </p>
        <div class="invite-actions">
          <button
            class="btn btn-danger"
            type="button"
            onClick={() => {
              confirmRef.current?.close();
              removeConfirmed().catch(() => {});
            }}
            disabled={action !== null}
          >
            {action === "remove" ? "Removing…" : "Remove partner"}
          </button>
          <button
            class="btn btn-ghost"
            type="button"
            onClick={() => confirmRef.current?.close()}
            disabled={action !== null}
          >
            Cancel
          </button>
        </div>
      </dialog>
    </div>
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
  const [error, setError] = useState<string | null>(null);
  const confirmRef = useRef<HTMLDialogElement>(null);
  const partnerName = partner.user.name ?? partner.user.email;

  async function removeConfirmed() {
    setAction("remove");
    setError(null);
    try {
      await ("digest_cadence" in partner
        ? onRemoveWatching(partner.id)
        : onRemoveWatcher(partner.id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to remove partner");
      setAction(null);
    }
  }

  return (
    <div class={`card${isWatching ? "" : " partner-card-compact"}`}>
      <div class="card-header">
        <span class="card-name">{partner.user.name ?? partner.user.email}</span>
        <span class="badge badge-green">Accepted</span>
      </div>
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
      {error && <p class="alert-error">{error}</p>}
      <div class="card-actions">
        {isWatching && partner.user.id && (
          <button
            class="btn btn-ghost"
            type="button"
            onClick={() => route(`/logs?user=${partner.user.id}`)}
          >
            View logs
          </button>
        )}
        <button
          class="btn btn-danger"
          type="button"
          onClick={() => confirmRef.current?.showModal()}
          disabled={action !== null}
        >
          {action === "remove" ? "Removing…" : "Remove"}
        </button>
      </div>
      <dialog ref={confirmRef}>
        <h3 class="dialog-title">Remove {partnerName}?</h3>
        <p class="invite-desc">
          This will remove your partner relationship with this person. The
          partner will be notified.
        </p>
        <div class="invite-actions">
          <button
            class="btn btn-danger"
            type="button"
            onClick={() => {
              confirmRef.current?.close();
              removeConfirmed().catch(() => {});
            }}
            disabled={action !== null}
          >
            {action === "remove" ? "Removing…" : "Remove partner"}
          </button>
          <button
            class="btn btn-ghost"
            type="button"
            onClick={() => confirmRef.current?.close()}
            disabled={action !== null}
          >
            Cancel
          </button>
        </div>
      </dialog>
    </div>
  );
}

function DeviceCard({
  device,
  onUpdateDevice,
  onRemoveDevice,
}: {
  device: Device;
  onUpdateDevice: (
    id: string,
    patch: { name?: string; enabled?: boolean },
  ) => Promise<void>;
  onRemoveDevice: (id: string) => Promise<void>;
}) {
  const { route } = useLocation();
  const [name, setName] = useState(device.name);
  const [enabled, setEnabled] = useState(device.enabled);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  function openEdit() {
    setName(device.name);
    setEnabled(device.enabled);
    setError(null);
    dialogRef.current?.showModal();
  }

  function closeEdit() {
    setError(null);
    dialogRef.current?.close();
  }

  function openDeleteDialog() {
    setError(null);
    dialogRef.current?.close();
    deleteDialogRef.current?.showModal();
  }

  function closeDeleteDialog() {
    setError(null);
    deleteDialogRef.current?.close();
  }

  async function handleSave(e: Event) {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      await onUpdateDevice(device.id, { name, enabled });
      dialogRef.current?.close();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save");
    } finally {
      setSaving(false);
    }
  }

  async function handleDeleteConfirmed() {
    setDeleting(true);
    setError(null);
    try {
      await onRemoveDevice(device.id);
      closeDeleteDialog();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete device");
    } finally {
      setDeleting(false);
    }
  }

  return (
    <div class="card">
      <div class="card-header">
        <span class="card-name">{device.name}</span>
        <span
          class={`badge ${device.status === "online" ? "badge-green" : "badge-gray"}`}
        >
          {device.status === "online" ? "Online" : "Offline"}
        </span>
      </div>
      <dl class="card-meta">
        <dt>Platform</dt>
        <dd>{device.platform}</dd>
        <dt>Last upload</dt>
        <dd>{formatRelativeTimestamp(device.last_upload_at)}</dd>
        {!device.enabled && (
          <>
            <dt>Status</dt>
            <dd class="muted">Disabled</dd>
          </>
        )}
      </dl>
      <div class="card-actions">
        <button
          class="btn btn-ghost"
          type="button"
          onClick={() => route(`/logs?device_id=${device.id}`)}
        >
          View logs
        </button>
        <button class="btn btn-ghost" type="button" onClick={openEdit}>
          Edit
        </button>
      </div>

      <dialog ref={dialogRef}>
        <h3 class="dialog-title">Edit device</h3>
        <form onSubmit={handleSave}>
          <div class="field">
            <label for={`device-name-${device.id}`}>Name</label>
            <input
              id={`device-name-${device.id}`}
              type="text"
              value={name}
              onInput={(e) => setName((e.target as HTMLInputElement).value)}
              required
            />
          </div>
          <label class="checkbox-label">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) =>
                setEnabled((e.target as HTMLInputElement).checked)
              }
            />
            Enabled
          </label>
          {error && <p class="alert-error">{error}</p>}
          <div class="invite-actions">
            <button
              class="btn btn-primary"
              type="submit"
              disabled={saving || deleting}
            >
              {saving ? "Saving…" : "Save"}
            </button>
            <button
              class="btn btn-danger"
              type="button"
              onClick={openDeleteDialog}
              disabled={saving || deleting}
            >
              Delete device
            </button>
            <button
              class="btn btn-ghost"
              type="button"
              onClick={closeEdit}
              disabled={saving || deleting}
            >
              Cancel
            </button>
          </div>
        </form>
      </dialog>

      <dialog ref={deleteDialogRef}>
        <h3 class="dialog-title">Delete device</h3>
        <p class="invite-desc">
          Delete "{device.name}"? This permanently removes its logs and uploads,
          and your partners will be notified.
        </p>
        {error && <p class="alert-error">{error}</p>}
        <div class="invite-actions">
          <button
            class="btn btn-danger"
            type="button"
            onClick={() => handleDeleteConfirmed().catch(() => {})}
            disabled={saving || deleting}
          >
            {deleting ? "Deleting…" : "Delete device"}
          </button>
          <button
            class="btn btn-ghost"
            type="button"
            onClick={closeDeleteDialog}
            disabled={saving || deleting}
          >
            Cancel
          </button>
        </div>
      </dialog>
    </div>
  );
}
