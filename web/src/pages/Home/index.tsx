import { useState, useEffect } from 'preact/hooks';
import { useAuth } from '../../context/auth';
import { api, Device, Partner } from '../../api';
import './style.css';

export function Home() {
  const { token } = useAuth();
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [partners, setPartners] = useState<Partner[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!token) return;
    Promise.all([api.getDevices(token), api.getPartners(token)])
      .then(([d, p]) => { setDevices(d); setPartners(p); })
      .catch((err) => setError(err instanceof Error ? err.message : 'Failed to load data'));
  }, [token]);

  return (
    <div class="dashboard">
      {error && <p class="error-banner">{error}</p>}

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
        <h2>Partners</h2>
        {partners === null ? (
          <p class="loading">Loading…</p>
        ) : partners.length === 0 ? (
          <p class="empty">No accountability partners yet.</p>
        ) : (
          <div class="card-grid">
            {partners.map((p) => <PartnerCard key={p.id} partner={p} />)}
          </div>
        )}
      </section>
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

function PartnerCard({ partner }: { partner: Partner }) {
  const pending = partner.status === 'pending';
  return (
    <div class="card">
      <div class="card-header">
        <span class="card-name">{partner.partner_email}</span>
        <span class={`badge ${pending ? 'badge-yellow' : 'badge-green'}`}>
          {pending ? 'Pending' : 'Active'}
        </span>
      </div>
      <dl class="card-meta">
        <dt>Role</dt>
        <dd>{partner.role === 'owner' ? 'You invited them' : 'They invited you'}</dd>
        <dt>Can view images</dt>
        <dd>{partner.permissions.view_images ? 'Yes' : 'No'}</dd>
        <dt>Can view logs</dt>
        <dd>{partner.permissions.view_logs ? 'Yes' : 'No'}</dd>
        <dt>Since</dt>
        <dd>{new Date(partner.created_at).toLocaleDateString()}</dd>
      </dl>
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

