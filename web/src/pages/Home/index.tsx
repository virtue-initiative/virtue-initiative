import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { api, Device, WatchingPartner, WatcherPartner } from "../../api";
import { Button } from "../../components/Button";
import { useAuth } from "../../context/auth";
import { removeDeviceFromCachedDataFeed } from "../../data-cache";
import { PARTNERS_CHANGED_EVENT } from "../../events";
import { formatRelativeTimestamp } from "../../utils/time";
import "./style.css";

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
  const { token, userId } = useAuth();
  const [devices, setDevices] = useState<Device[]>([]);
  const [watching, setWatching] = useState<WatchingPartner[]>([]);
  const [watchers, setWatchers] = useState<WatcherPartner[]>([]);
  const [error, setError] = useState<string | null>(null);

  function reload() {
    if (!token) return;
    Promise.all([api.getDevices(token), api.getPartners(token)])
      .then(([deviceList, partnerList]) => {
        setDevices(deviceList);
        setWatching(partnerList.watching);
        setWatchers(partnerList.watchers);
      })
      .catch((err) =>
        setError(
          err instanceof Error ? err.message : "Failed to load dashboard",
        ),
      );
  }

  useEffect(reload, [token]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const handler = () => reload();
    window.addEventListener(PARTNERS_CHANGED_EVENT, handler);
    return () => window.removeEventListener(PARTNERS_CHANGED_EVENT, handler);
  }, [token]);

  const ownDevices = useMemo(
    () => devices.filter((device) => device.owner === userId),
    [devices, userId],
  );
  const devicesByOwner = useMemo(() => {
    const map = new Map<string, Device[]>();
    for (const device of devices) {
      const ownerDevices = map.get(device.owner) ?? [];
      ownerDevices.push(device);
      map.set(device.owner, ownerDevices);
    }
    return map;
  }, [devices]);
  const acceptedWatching = useMemo(
    () => watching.filter((partner) => partner.status === "accepted"),
    [watching],
  );
  const pendingWatching = useMemo(
    () => watching.filter((partner) => partner.status === "pending"),
    [watching],
  );
  const acceptedWatchers = useMemo(
    () => watchers.filter((partner) => partner.status === "accepted"),
    [watchers],
  );
  const pendingWatchers = useMemo(
    () => watchers.filter((partner) => partner.status === "pending"),
    [watchers],
  );

  return (
    <div class="dashboard">
      {error && <p class="alert-error">{error}</p>}

      <section class="dash-section">
        <div class="section-header">
          <h2>My devices</h2>
          <a
            class="btn btn-primary"
            href="https://virtueinitiative.org/help/installation/"
          >
            Create device
          </a>
        </div>
        {ownDevices.length === 0 ? (
          <p class="empty">No devices</p>
        ) : (
          <div class="card-grid">
            {ownDevices.map((device) => (
              <DeviceCard
                key={device.id}
                device={device}
                token={token!}
                viewerUserId={userId!}
                onChanged={reload}
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
          token={token!}
          onChanged={reload}
        />
      </section>

      <section class="dash-section">
        <div class="section-header">
          <h2>People who can monitor you</h2>
          <InviteButton token={token!} onInvited={reload} />
        </div>
        <PartnerArea
          emptyLabel="No one can monitor you yet."
          pending={pendingWatchers}
          accepted={acceptedWatchers}
          partnerDevicesByOwner={devicesByOwner}
          token={token!}
          onChanged={reload}
        />
      </section>
    </div>
  );
}

function PartnerArea({
  emptyLabel,
  pending,
  accepted,
  partnerDevicesByOwner,
  token,
  onChanged,
}: {
  emptyLabel: string;
  pending: Array<WatchingPartner | WatcherPartner>;
  accepted: Array<WatchingPartner | WatcherPartner>;
  partnerDevicesByOwner: Map<string, Device[]>;
  token: string;
  onChanged: () => void;
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
                token={token}
                onChanged={onChanged}
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
                token={token}
                onChanged={onChanged}
              />
            ),
          )}
        </div>
      )}
    </section>
  );
}

function InviteButton({
  token,
  onInvited,
}: {
  token: string;
  onInvited: () => void;
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
      await api.invitePartner(token, email);
      close();
      onInvited();
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
  token,
  onChanged,
}: {
  partner: WatchingPartner | WatcherPartner;
  token: string;
  onChanged: () => void;
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
        ? api.deleteWatching(token, partner.id)
        : api.deleteWatcher(token, partner.id));
      onChanged();
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
  token,
  onChanged,
}: {
  partner: WatchingPartner | WatcherPartner;
  devices: Device[];
  token: string;
  onChanged: () => void;
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
        ? api.deleteWatching(token, partner.id)
        : api.deleteWatcher(token, partner.id));
      onChanged();
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
  token,
  viewerUserId,
  onChanged,
}: {
  device: Device;
  token: string;
  viewerUserId: string;
  onChanged: () => void;
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
      await api.patchDevice(token, device.id, { name, enabled });
      dialogRef.current?.close();
      onChanged();
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
      await api.deleteDevice(token, device.id);
      await removeDeviceFromCachedDataFeed(
        viewerUserId,
        viewerUserId,
        device.id,
      ).catch((err) => {
        console.warn("[home] failed to remove deleted device from cache", err);
      });
      closeDeleteDialog();
      onChanged();
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
