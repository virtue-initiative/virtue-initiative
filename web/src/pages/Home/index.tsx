import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { api, Device, WatchingPartner, WatcherPartner } from "../../api";
import { Button } from "../../components/Button";
import { useAuth } from "../../context/auth";
import { formatDate, formatRelativeTimestamp } from "../../utils/time";
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

function relationshipSummary(partner: WatchingPartner | WatcherPartner) {
  return "user" in partner && "digest_cadence" in partner
    ? "You are monitoring this person."
    : "This person is monitoring you.";
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

  const ownDevices = useMemo(
    () => devices.filter((device) => device.owner === userId),
    [devices, userId],
  );
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
        </div>
        {ownDevices.length === 0 ? (
          <p class="empty">No devices registered yet.</p>
        ) : (
          <div class="card-grid">
            {ownDevices.map((device) => (
              <DeviceCard
                key={device.id}
                device={device}
                token={token!}
                onChanged={reload}
              />
            ))}
          </div>
        )}
      </section>

      <section class="dash-section">
        <div class="section-header">
          <h2>Partners</h2>
          <InviteButton token={token!} onInvited={reload} />
        </div>

        {watching.length === 0 && watchers.length === 0 ? (
          <p class="empty">No accountability partners yet.</p>
        ) : (
          <div class="partners-split">
            <PartnerArea
              title="Watching"
              subtitle="People you can monitor and review."
              pending={pendingWatching}
              accepted={acceptedWatching}
              token={token!}
              onChanged={reload}
            />
            <PartnerArea
              title="Watchers"
              subtitle="People who can monitor your account."
              pending={pendingWatchers}
              accepted={acceptedWatchers}
              token={token!}
              onChanged={reload}
            />
          </div>
        )}
      </section>

      {acceptedWatching
        .filter((partner) => partner.user.id)
        .map((partner) => (
          <PartnerDevicesSection
            key={partner.id}
            partner={partner}
            devices={devices.filter(
              (device) => device.owner === partner.user.id,
            )}
          />
        ))}
    </div>
  );
}

function PartnerArea({
  title,
  subtitle,
  pending,
  accepted,
  token,
  onChanged,
}: {
  title: string;
  subtitle: string;
  pending: Array<WatchingPartner | WatcherPartner>;
  accepted: Array<WatchingPartner | WatcherPartner>;
  token: string;
  onChanged: () => void;
}) {
  return (
    <section class="partners-panel">
      <div class="partners-panel-header">
        <h3>{title}</h3>
        <p>{subtitle}</p>
      </div>

      {pending.length === 0 && accepted.length === 0 ? (
        <p class="empty">{`No ${title.toLowerCase()} relationships yet.`}</p>
      ) : (
        <>
          {pending.length > 0 && (
            <>
              <p class="partners-group-label">Pending</p>
              <div class="card-grid">
                {pending.map((partner) => (
                  <PendingPartnerCard
                    key={partner.id}
                    partner={partner}
                    token={token}
                    onChanged={onChanged}
                  />
                ))}
              </div>
            </>
          )}

          {accepted.length > 0 && (
            <>
              <p class="partners-group-label">Accepted</p>
              <div class="card-grid">
                {accepted.map((partner) => (
                  <PartnerCard
                    key={partner.id}
                    partner={partner}
                    token={token}
                    onChanged={onChanged}
                  />
                ))}
              </div>
            </>
          )}
        </>
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

  async function remove() {
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
      <p class="invite-desc">
        {"digest_cadence" in partner
          ? "You have been invited to monitor this person. Once you accept, you will be able to view their encrypted activity data."
          : "You invited this person to monitor your account. Once they accept and have an account, new uploads will include them automatically."}
      </p>
      <dl class="card-meta">
        <dt>Email</dt>
        <dd>{partner.user.email}</dd>
        <dt>Relationship</dt>
        <dd>{relationshipSummary(partner)}</dd>
        <dt>Created</dt>
        <dd>{formatDate(partner.created_at ?? Date.now())}</dd>
      </dl>
      {error && <p class="alert-error">{error}</p>}
      <div class="card-actions">
        <button
          class="btn btn-danger"
          type="button"
          onClick={remove}
          disabled={action !== null}
        >
          {action === "remove" ? "Removing…" : "Remove"}
        </button>
      </div>
    </div>
  );
}

function PartnerCard({
  partner,
  token,
  onChanged,
}: {
  partner: WatchingPartner | WatcherPartner;
  token: string;
  onChanged: () => void;
}) {
  const { route } = useLocation();
  const [action, setAction] = useState<"remove" | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function remove() {
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
    <div class="card">
      <div class="card-header">
        <span class="card-name">{partner.user.name ?? partner.user.email}</span>
        <span class="badge badge-green">Accepted</span>
      </div>
      <dl class="card-meta">
        <dt>Email</dt>
        <dd>{partner.user.email}</dd>
        <dt>Relationship</dt>
        <dd>{relationshipSummary(partner)}</dd>
      </dl>
      {error && <p class="alert-error">{error}</p>}
      <div class="card-actions">
        {"digest_cadence" in partner && partner.user.id && (
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
          onClick={remove}
          disabled={action !== null}
        >
          {action === "remove" ? "Removing…" : "Remove"}
        </button>
      </div>
    </div>
  );
}

function PartnerDevicesSection({
  partner,
  devices,
}: {
  partner: WatchingPartner;
  devices: Device[];
}) {
  const { route } = useLocation();
  const partnerId = partner.user.id;

  return (
    <section class="dash-section">
      <div class="section-header">
        <h2>{partner.user.name ?? partner.user.email}</h2>
      </div>

      {devices.length === 0 ? (
        <p class="empty">No devices registered.</p>
      ) : (
        <div class="card-grid">
          {devices.map((device) => (
            <div class="card" key={device.id}>
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
              </dl>
              <div class="card-actions">
                <button
                  class="btn btn-ghost"
                  type="button"
                  onClick={() =>
                    route(`/logs?user=${partnerId}&device_id=${device.id}`)
                  }
                >
                  View logs
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function DeviceCard({
  device,
  token,
  onChanged,
}: {
  device: Device;
  token: string;
  onChanged: () => void;
}) {
  const { route } = useLocation();
  const [name, setName] = useState(device.name);
  const [enabled, setEnabled] = useState(device.enabled);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);

  function openEdit() {
    setName(device.name);
    setEnabled(device.enabled);
    setError(null);
    dialogRef.current?.showModal();
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

  async function handleDelete() {
    if (
      !confirm(
        `Delete device "${device.name}"? This removes its logs and uploads.`,
      )
    ) {
      return;
    }

    setDeleting(true);
    setError(null);
    try {
      await api.deleteDevice(token, device.id);
      dialogRef.current?.close();
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
              onClick={handleDelete}
              disabled={saving || deleting}
            >
              {deleting ? "Deleting…" : "Delete device"}
            </button>
            <button
              class="btn btn-ghost"
              type="button"
              onClick={() => dialogRef.current?.close()}
              disabled={saving || deleting}
            >
              Cancel
            </button>
          </div>
        </form>
      </dialog>
    </div>
  );
}
