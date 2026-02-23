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
        <h2>Devices</h2>
        {devices === null ? (
          <p class="loading">Loading…</p>
        ) : devices.length === 0 ? (
          <p class="empty">No devices registered yet.</p>
        ) : (
          <div class="card-grid">
            {devices.map((d) => <DeviceCard key={d.id} device={d} />)}
          </div>
        )}
      </section>

      <section class="dash-section">
        <div class="section-header">
          <h2>Partners</h2>
          <InviteButton token={token!} onInvited={reload} />
        </div>
        {partners === null ? (
          <p class="loading">Loading…</p>
        ) : (
          <PartnersList partners={partners} token={token!} onChanged={reload} />
        )}
      </section>
    </div>
  );
}

function PartnersList({
  partners,
  token,
  onChanged,
}: {
  partners: Partner[];
  token: string;
  onChanged: () => void;
}) {
  // role='owner': you invited them → they monitor you
  // role='partner': they invited you → you monitor them
  const monitoringYou = partners.filter((p) => p.role === 'owner' && p.status === 'accepted');
  const youMonitor = partners.filter((p) => p.role === 'partner' && p.status === 'accepted');

  if (monitoringYou.length === 0 && youMonitor.length === 0) {
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
      {youMonitor.length > 0 && (
        <div>
          <p class="partners-group-label">You're monitoring</p>
          <div class="card-grid">
            {youMonitor.map((p) => (
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

function DeviceCard({ device }: { device: Device }) {
  const online = device.status === 'online';
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
  const dialogRef = useRef<HTMLDialogElement>(null);

  async function confirmDelete() {
    setLoading(true);
    setError(null);
    try {
      await api.deletePartner(token, partner.id);
      dialogRef.current?.close();
      onDeleted();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to remove partner');
      setLoading(false);
    }
  }

  return (
    <div class="card">
      <div class="card-header">
        <span class="card-name">{partner.partner_email}</span>
        <span class="badge badge-green">Active</span>
      </div>
      <dl class="card-meta">
        <dt>Can view images</dt>
        <dd>{partner.permissions.view_images ? 'Yes' : 'No'}</dd>
        <dt>Can view logs</dt>
        <dd>{partner.permissions.view_logs ? 'Yes' : 'No'}</dd>
        <dt>Since</dt>
        <dd>{new Date(partner.created_at).toLocaleDateString()}</dd>
      </dl>
      <button
        class="btn btn-danger btn-sm card-delete"
        type="button"
        onClick={() => dialogRef.current?.showModal()}
      >
        Remove
      </button>

      <dialog ref={dialogRef} class="invite-dialog" onClick={(e) => { if (e.target === dialogRef.current) dialogRef.current?.close(); }}>
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
          <button class="btn btn-ghost btn-sm" type="button" onClick={() => { dialogRef.current?.close(); setError(null); }}>
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
