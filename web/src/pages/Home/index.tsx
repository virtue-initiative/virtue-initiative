import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useLocation } from "preact-iso";
import { api, Device, Partner } from "../../api";
import { Button } from "../../components/Button";
import { useAuth } from "../../context/auth";
import { useE2EE } from "../../context/e2ee";
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

export function Home() {
  const { token, userId } = useAuth();
  const [devices, setDevices] = useState<Device[]>([]);
  const [partners, setPartners] = useState<Partner[]>([]);
  const [error, setError] = useState<string | null>(null);

  function reload() {
    if (!token) return;
    Promise.all([api.getDevices(token), api.getPartners(token)])
      .then(([deviceList, partnerList]) => {
        setDevices(deviceList);
        setPartners(partnerList);
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
  const acceptedPartners = useMemo(
    () => partners.filter((partner) => partner.status === "accepted"),
    [partners],
  );
  const pendingPartners = useMemo(
    () => partners.filter((partner) => partner.status === "pending"),
    [partners],
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

        {pendingPartners.length === 0 && acceptedPartners.length === 0 ? (
          <p class="empty">No accountability partners yet.</p>
        ) : (
          <>
            {pendingPartners.length > 0 && (
              <>
                <p class="partners-group-label">Pending relationships</p>
                <div class="card-grid">
                  {pendingPartners.map((partner) => (
                    <PendingPartnerCard
                      key={partner.id}
                      partner={partner}
                      token={token!}
                      onChanged={reload}
                    />
                  ))}
                </div>
              </>
            )}

            {acceptedPartners.length > 0 && (
              <>
                <p class="partners-group-label">Accepted partners</p>
                <div class="card-grid">
                  {acceptedPartners.map((partner) => (
                    <PartnerCard
                      key={partner.id}
                      partner={partner}
                      token={token!}
                      onChanged={reload}
                    />
                  ))}
                </div>
              </>
            )}
          </>
        )}
      </section>

      {acceptedPartners
        .filter((partner) => partner.partner.id)
        .map((partner) => (
          <PartnerDevicesSection
            key={partner.id}
            partner={partner}
            devices={devices.filter(
              (device) => device.owner === partner.partner.id,
            )}
          />
        ))}
    </div>
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
  const [viewData, setViewData] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);

  function open() {
    setEmail("");
    setViewData(true);
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
      await api.invitePartner(token, email, { view_data: viewData });
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
      <Button
        className="btn-primary btn-sm"
        onClick={open}
        icon={<UserPlusIcon />}
      >
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
          <label class="checkbox-label">
            <input
              type="checkbox"
              checked={viewData}
              onChange={(e) =>
                setViewData((e.target as HTMLInputElement).checked)
              }
            />
            Can view data
          </label>
          {error && <p class="alert-error">{error}</p>}
          <div class="invite-actions">
            <button
              class="btn btn-primary btn-sm"
              type="submit"
              disabled={loading}
            >
              {loading ? "Sending…" : "Send invite"}
            </button>
            <button class="btn btn-ghost btn-sm" type="button" onClick={close}>
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
  partner: Partner;
  token: string;
  onChanged: () => void;
}) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function accept() {
    setLoading(true);
    setError(null);
    try {
      await api.acceptPartner(token, partner.id);
      onChanged();
    } catch (err) {
      setError(
        err instanceof Error
          ? err.message
          : "Only the invited partner can accept this request.",
      );
      setLoading(false);
    }
  }

  async function remove() {
    setLoading(true);
    setError(null);
    try {
      await api.deletePartner(token, partner.id);
      onChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to remove request");
      setLoading(false);
    }
  }

  return (
    <div class="card card-highlight">
      <div class="card-header">
        <span class="card-name">
          {partner.partner.name ?? partner.partner.email}
        </span>
        <span class="badge badge-yellow">Pending</span>
      </div>
      <p class="invite-desc">
        {partner.permissions.view_data
          ? "This relationship includes access to encrypted activity data."
          : "This relationship does not include data access."}
      </p>
      <dl class="card-meta">
        <dt>Email</dt>
        <dd>{partner.partner.email}</dd>
        <dt>Created</dt>
        <dd>{formatDate(partner.created_at)}</dd>
      </dl>
      {error && <p class="alert-error">{error}</p>}
      <div class="card-actions">
        <button
          class="btn btn-primary btn-sm"
          type="button"
          onClick={accept}
          disabled={loading}
        >
          {loading ? "Working…" : "Accept"}
        </button>
        <button
          class="btn btn-danger btn-sm"
          type="button"
          onClick={remove}
          disabled={loading}
        >
          Remove
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
  partner: Partner;
  token: string;
  onChanged: () => void;
}) {
  const { route } = useLocation();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function remove() {
    setLoading(true);
    setError(null);
    try {
      await api.deletePartner(token, partner.id);
      onChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to remove partner");
      setLoading(false);
    }
  }

  return (
    <div class="card">
      <div class="card-header">
        <span class="card-name">
          {partner.partner.name ?? partner.partner.email}
        </span>
        <span class="badge badge-green">Accepted</span>
      </div>
      <dl class="card-meta">
        <dt>Email</dt>
        <dd>{partner.partner.email}</dd>
        <dt>Can view data</dt>
        <dd>{partner.permissions.view_data ? "Yes" : "No"}</dd>
      </dl>
      {error && <p class="alert-error">{error}</p>}
      <div class="card-actions">
        {partner.partner.id && partner.permissions.view_data && (
          <button
            class="btn btn-ghost btn-sm"
            type="button"
            onClick={() => route(`/logs?user=${partner.partner.id}`)}
          >
            View logs
          </button>
        )}
        <button
          class="btn btn-danger btn-sm"
          type="button"
          onClick={remove}
          disabled={loading}
        >
          {loading ? "Removing…" : "Remove"}
        </button>
      </div>
    </div>
  );
}

function PartnerDevicesSection({
  partner,
  devices,
}: {
  partner: Partner;
  devices: Device[];
}) {
  const { route } = useLocation();
  const e2ee = useE2EE();
  const [password, setPassword] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const partnerId = partner.partner.id;
  const hasKey = partnerId ? Boolean(e2ee.getKey(partnerId)) : false;

  async function saveKey(e: Event) {
    e.preventDefault();
    if (!partnerId) return;
    setSaving(true);
    setStatus(null);
    try {
      await e2ee.setKey(password, partnerId);
      setPassword("");
      setStatus("Shared decryption password saved locally.");
    } catch (err) {
      setStatus(err instanceof Error ? err.message : "Failed to save password");
    } finally {
      setSaving(false);
    }
  }

  return (
    <section class="dash-section">
      <div class="section-header">
        <h2>{partner.partner.name ?? partner.partner.email}</h2>
      </div>

      {partner.permissions.view_data && partnerId && !hasKey && (
        <form class="card settings-form" onSubmit={saveKey}>
          <p class="settings-hint">
            Enter this partner's shared E2EE password to decrypt their uploaded
            blocks.
          </p>
          <div class="field">
            <label for={`partner-key-${partner.id}`}>
              Shared E2EE password
            </label>
            <input
              id={`partner-key-${partner.id}`}
              type="password"
              value={password}
              onInput={(e) => setPassword((e.target as HTMLInputElement).value)}
              placeholder="Shared encryption password"
              required
            />
          </div>
          {status && (
            <p class={status.endsWith(".") ? "alert-success" : "alert-error"}>
              {status}
            </p>
          )}
          <button
            class="btn btn-primary btn-sm"
            type="submit"
            disabled={saving}
          >
            {saving ? "Saving…" : "Save password"}
          </button>
        </form>
      )}

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
              {partnerId && partner.permissions.view_data && (
                <div class="card-actions">
                  <button
                    class="btn btn-ghost btn-sm"
                    type="button"
                    onClick={() =>
                      route(`/logs?user=${partnerId}&device_id=${device.id}`)
                    }
                  >
                    View logs
                  </button>
                </div>
              )}
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
          class="btn btn-ghost btn-sm"
          type="button"
          onClick={() => route(`/logs?device_id=${device.id}`)}
        >
          View logs
        </button>
        <button class="btn btn-ghost btn-sm" type="button" onClick={openEdit}>
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
              class="btn btn-primary btn-sm"
              type="submit"
              disabled={saving}
            >
              {saving ? "Saving…" : "Save"}
            </button>
            <button
              class="btn btn-ghost btn-sm"
              type="button"
              onClick={() => dialogRef.current?.close()}
            >
              Cancel
            </button>
          </div>
        </form>
      </dialog>
    </div>
  );
}
