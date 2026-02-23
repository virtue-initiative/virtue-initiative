import { useState, useEffect, useRef } from 'preact/hooks';
import { useAuth } from '../../context/auth';
import { api, Device, Partner } from '../../api';
import './style.css';

export function Home() {
  const { token } = useAuth();
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [partners, setPartners] = useState<Partner[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  function reload() {
    if (!token) return;
    Promise.all([api.getDevices(token), api.getPartners(token)])
      .then(([d, p]) => { setDevices(d); setPartners(p); })
      .catch((err) => setError(err instanceof Error ? err.message : 'Failed to load data'));
  }

  useEffect(reload, [token]);

  const pendingInvites = partners?.filter(
    (p) => p.role === 'partner' && p.status === 'pending',
  ) ?? [];

  const monitoringYou = partners?.filter((p) => p.role === 'owner' && p.status === 'accepted') ?? [];
  const youMonitor = partners?.filter((p) => p.role === 'partner' && p.status === 'accepted') ?? [];

  return (
    <div class="dashboard">
      {error && <p class="error-banner">{error}</p>}

      {pendingInvites.length > 0 && (
        <section class="dash-section">
          <h2>Pending invites</h2>
          <div class="card-grid">
            {pendingInvites.map((p) => (
              <PendingInviteCard key={p.id} partner={p} token={token!} onAccepted={reload} />
            ))}
          </div>
        </section>
      )}

      <section class="dash-section">
        <h2>My devices</h2>
        {devices === null ? (
          <p class="loading">Loading…</p>
        ) : devices.length === 0 ? (
          <p class="empty">No devices registered yet.</p>
        ) : (
          <div class="card-grid">
            {devices.map((d) => <DeviceCard key={d.id} device={d} token={token!} onChanged={reload} />)}
          </div>
        )}
      </section>

      {youMonitor.length > 0 && youMonitor.map((p) => (
        <PartnerDevicesSection key={p.id} partner={p} token={token!} />
      ))}

      <section class="dash-section">
        <div class="section-header">
          <h2>Partners</h2>
          <InviteButton token={token!} onInvited={reload} />
        </div>
        {partners === null ? (
          <p class="loading">Loading…</p>
        ) : (
          <PartnersList
            monitoringYou={monitoringYou}
            youMonitor={youMonitor}
            pendingInvites={pendingInvites}
            token={token!}
            onChanged={reload}
          />
        )}
      </section>
    </div>
  );
}

function PartnersList({
  monitoringYou,
  youMonitor,
  pendingInvites,
  token,
  onChanged,
}: {
  monitoringYou: Partner[];
  youMonitor: Partner[];
  pendingInvites: Partner[];
  token: string;
  onChanged: () => void;
}) {
  if (monitoringYou.length === 0 && youMonitor.length === 0 && pendingInvites.length === 0) {
    return <p class="empty">No accountability partners yet.</p>;
  }

  return (
    <div class="partners-split">
      {monitoringYou.length > 0 && (
        <div>
          <p class="partners-group-label">Monitoring you</p>
          <div class="card-grid">
            {monitoringYou.map((p) => (
              <PartnerCard key={p.id} partner={p} token={token} onDeleted={onChanged} />
            ))}
          </div>
        </div>
      )}
      {pendingInvites.length > 0 && (
        <div>
          <p class="partners-group-label">Pending invites</p>
          <div class="card-grid">
            {pendingInvites.map((p) => (
              <PartnerCard key={p.id} partner={p} token={token} onDeleted={onChanged} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function InviteButton({ token, onInvited }: { token: string; onInvited: () => void }) {
  const [email, setEmail] = useState('');
  const [viewImages, setViewImages] = useState(true);
  const [viewLogs, setViewLogs] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);

  function open() {
    setEmail('');
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
      await api.invitePartner(token, email, { view_images: viewImages, view_logs: viewLogs });
      dialogRef.current?.close();
      onInvited();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to send invite');
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <button class="btn btn-primary btn-sm" onClick={open} type="button">
        + Invite partner
      </button>
      <dialog ref={dialogRef} class="invite-dialog" onClick={(e) => { if (e.target === dialogRef.current) close(); }}>
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
          <div class="invite-perms">
            <label class="checkbox-label">
              <input type="checkbox" checked={viewImages} onChange={(e) => setViewImages((e.target as HTMLInputElement).checked)} />
              Can view images
            </label>
            <label class="checkbox-label">
              <input type="checkbox" checked={viewLogs} onChange={(e) => setViewLogs((e.target as HTMLInputElement).checked)} />
              Can view logs
            </label>
          </div>
          {error && <p class="form-error">{error}</p>}
          <div class="invite-actions">
            <button class="btn btn-primary btn-sm" type="submit" disabled={loading}>
              {loading ? 'Sending…' : 'Send invite'}
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

function PendingInviteCard({
  partner,
  token,
  onAccepted,
}: {
  partner: Partner;
  token: string;
  onAccepted: () => void;
}) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function accept() {
    setLoading(true);
    setError(null);
    try {
      await api.acceptPartner(token, partner.id);
      onAccepted();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to accept');
      setLoading(false);
    }
  }

  return (
    <div class="card card-highlight">
      <div class="card-header">
        <span class="card-name">{partner.partner_email}</span>
        <span class="badge badge-yellow">Invite</span>
      </div>
      <p class="invite-desc">
        Invited you as an accountability partner
        {partner.permissions.view_images && partner.permissions.view_logs
          ? ' with access to your images and logs.'
          : partner.permissions.view_images
          ? ' with access to your images.'
          : partner.permissions.view_logs
          ? ' with access to your logs.'
          : '.'}
      </p>
      {error && <p class="form-error">{error}</p>}
      <button class="btn btn-primary btn-sm" onClick={accept} disabled={loading} type="button">
        {loading ? 'Accepting…' : 'Accept invite'}
      </button>
    </div>
  );
}

function PartnerDevicesSection({ partner, token }: { partner: Partner; token: string }) {
  const [devices, setDevices] = useState<Device[] | null>(null);

  useEffect(() => {
    api.getDevices(token, { user: partner.partner_user_id })
      .then(setDevices)
      .catch(() => setDevices([]));
  }, [partner.partner_user_id, token]);

  return (
    <section class="dash-section">
      <div class="section-header">
        <h2>{partner.partner_email}'s devices</h2>
        <button
          class="btn btn-ghost btn-sm"
          type="button"
          onClick={() => window.location.href = `/logs?user=${partner.partner_user_id}`}
        >
          View logs
        </button>
      </div>
      {devices === null ? (
        <p class="loading">Loading…</p>
      ) : devices.length === 0 ? (
        <p class="empty">No devices registered.</p>
      ) : (
        <div class="card-grid">
          {devices.map((d) => (
            <div class="card" key={d.id}>
              <div class="card-header">
                <span class="card-name">{d.name}</span>
                <span class={`badge ${d.status === 'online' ? 'badge-green' : 'badge-gray'}`}>
                  {d.status === 'online' ? 'Online' : 'Offline'}
                </span>
              </div>
              <dl class="card-meta">
                <dt>Platform</dt><dd>{d.platform}</dd>
                <dt>Last seen</dt><dd>{d.last_seen_at ? relativeTime(d.last_seen_at) : 'Never'}</dd>
                <dt>Last upload</dt><dd>{d.last_upload_at ? relativeTime(d.last_upload_at) : 'Never'}</dd>
              </dl>
              <div class="card-actions">
                <button
                  class="btn btn-ghost btn-sm"
                  type="button"
                  onClick={() => window.location.href = `/logs?user=${partner.partner_user_id}&device_id=${d.id}`}
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

function DeviceCard({ device, token, onChanged }: { device: Device; token: string; onChanged: () => void }) {
  const online = device.status === 'online';
  const [name, setName] = useState(device.name);
  const [interval, setInterval] = useState(String(device.interval_seconds));
  const [enabled, setEnabled] = useState(device.enabled);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);

  function openEdit() {
    setName(device.name);
    setInterval(String(device.interval_seconds));
    setEnabled(device.enabled);
    setError(null);
    dialogRef.current?.showModal();
  }

  async function handleSave(e: Event) {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      await api.patchDevice(token, device.id, {
        name,
        interval_seconds: Number(interval),
        enabled,
      });
      dialogRef.current?.close();
      onChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save');
    } finally {
      setSaving(false);
    }
  }

  return (
    <div class="card">
      <div class="card-header">
        <span class="card-name">{device.name}</span>
        <span class={`badge ${online ? 'badge-green' : 'badge-gray'}`}>
          {online ? 'Online' : 'Offline'}
        </span>
      </div>
      <dl class="card-meta">
        <dt>Platform</dt>
        <dd>{device.platform}</dd>
        <dt>Interval</dt>
        <dd>{device.interval_seconds}s</dd>
        <dt>Last seen</dt>
        <dd>{device.last_seen_at ? relativeTime(device.last_seen_at) : 'Never'}</dd>
        <dt>Last upload</dt>
        <dd>{device.last_upload_at ? relativeTime(device.last_upload_at) : 'Never'}</dd>
        {!device.enabled && <><dt>Status</dt><dd class="muted">Disabled</dd></>}
      </dl>
      <div class="card-actions">
        <button
          class="btn btn-ghost btn-sm"
          type="button"
          onClick={() => window.location.href = `/logs?device_id=${device.id}`}
        >
          View logs
        </button>
        <button class="btn btn-ghost btn-sm" type="button" onClick={openEdit}>
          Edit
        </button>
      </div>

      <dialog ref={dialogRef} class="invite-dialog" onClick={(e) => { if (e.target === dialogRef.current) dialogRef.current?.close(); }}>
        <h3 class="dialog-title">Edit device</h3>
        <form onSubmit={handleSave}>
          <div class="field">
            <label for="device-name">Name</label>
            <input
              id="device-name"
              type="text"
              value={name}
              onInput={(e) => setName((e.target as HTMLInputElement).value)}
              required
            />
          </div>
          <div class="field">
            <label for="device-interval">Capture interval (seconds)</label>
            <input
              id="device-interval"
              type="number"
              min="15"
              value={interval}
              onInput={(e) => setInterval((e.target as HTMLInputElement).value)}
              required
            />
          </div>
          <label class="checkbox-label">
            <input type="checkbox" checked={enabled} onChange={(e) => setEnabled((e.target as HTMLInputElement).checked)} />
            Enabled
          </label>
          {error && <p class="form-error">{error}</p>}
          <div class="invite-actions">
            <button class="btn btn-primary btn-sm" type="submit" disabled={saving}>
              {saving ? 'Saving…' : 'Save'}
            </button>
            <button class="btn btn-ghost btn-sm" type="button" onClick={() => dialogRef.current?.close()}>
              Cancel
            </button>
          </div>
        </form>
      </dialog>
    </div>
  );
}

function PartnerCard({
  partner,
  token,
  onDeleted,
}: {
  partner: Partner;
  token: string;
  onDeleted: () => void;
}) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);
  const infoDialogRef = useRef<HTMLDialogElement>(null);

  async function confirmDelete() {
    setLoading(true);
    setError(null);
    try {
      await api.deletePartner(token, partner.id);
      deleteDialogRef.current?.close();
      onDeleted();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to remove partner');
      setLoading(false);
    }
  }

  // role='owner': they monitor me (I invited them)
  // role='partner': I monitor them (they invited me)
  const isMonitoringMe = partner.role === 'owner';

  return (
    <div class="card">
      <div class="card-header">
        <span class="card-name">{partner.partner_email}</span>
        <span class="badge badge-green">Active</span>
      </div>
      <dl class="card-meta">
        <dt>Since</dt>
        <dd>{new Date(partner.created_at).toLocaleDateString()}</dd>
      </dl>
      <div class="card-actions">
        <button
          class="btn btn-ghost btn-sm"
          type="button"
          onClick={() => infoDialogRef.current?.showModal()}
        >
          Info
        </button>
        <button
          class="btn btn-danger btn-sm"
          type="button"
          onClick={() => deleteDialogRef.current?.showModal()}
        >
          Remove
        </button>
      </div>

      <dialog ref={infoDialogRef} class="invite-dialog" onClick={(e) => { if (e.target === infoDialogRef.current) infoDialogRef.current?.close(); }}>
        <h3 class="dialog-title">{partner.partner_email}</h3>
        <dl class="card-meta">
          <dt>Role</dt>
          <dd>{isMonitoringMe ? 'Monitoring you' : 'You are monitoring them'}</dd>
          <dt>Can view images</dt>
          <dd>{partner.permissions.view_images ? 'Yes' : 'No'}</dd>
          <dt>Can view logs</dt>
          <dd>{partner.permissions.view_logs ? 'Yes' : 'No'}</dd>
          <dt>Since</dt>
          <dd>{new Date(partner.created_at).toLocaleDateString()}</dd>
        </dl>
        <div class="invite-actions" style="margin-top:1rem">
          <button class="btn btn-ghost btn-sm" type="button" onClick={() => infoDialogRef.current?.close()}>
            Close
          </button>
        </div>
      </dialog>

      <dialog ref={deleteDialogRef} class="invite-dialog" onClick={(e) => { if (e.target === deleteDialogRef.current) deleteDialogRef.current?.close(); }}>
        <h3 class="dialog-title">Remove partner?</h3>
        <p class="invite-desc">
          Are you sure you want to remove <strong>{partner.partner_email}</strong> as an
          accountability partner? They will be notified by email.
        </p>
        {error && <p class="form-error">{error}</p>}
        <div class="invite-actions">
          <button class="btn btn-danger btn-sm" type="button" onClick={confirmDelete} disabled={loading}>
            {loading ? 'Removing…' : 'Yes, remove'}
          </button>
          <button class="btn btn-ghost btn-sm" type="button" onClick={() => { deleteDialogRef.current?.close(); setError(null); }}>
            Cancel
          </button>
        </div>
      </dialog>
    </div>
  );
}

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return 'Just now';
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}
