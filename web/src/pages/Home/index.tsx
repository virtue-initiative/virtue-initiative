import { useState, useEffect, useRef } from 'preact/hooks';
import { useLocation } from 'preact-iso';
import { useAuth } from '../../context/auth';
import { useE2EE } from '../../context/e2ee';
import { api, Device, Partner } from '../../api';
import { deriveKey, encryptData } from '../../crypto';
import './style.css';
import { Button } from '../../components/Button';

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
  const sentRequests = partners?.filter(
    (p) => p.role === 'owner' && p.status === 'pending',
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
            sentRequests={sentRequests}
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
  sentRequests,
  token,
  onChanged,
}: {
  monitoringYou: Partner[];
  youMonitor: Partner[];
  pendingInvites: Partner[];
  sentRequests: Partner[];
  token: string;
  onChanged: () => void;
}) {
  if (monitoringYou.length === 0 && youMonitor.length === 0 && pendingInvites.length === 0 && sentRequests.length === 0) {
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
      {sentRequests.length > 0 && (
        <div>
          <p class="partners-group-label">Sent requests</p>
          <div class="card-grid">
            {sentRequests.map((p) => (
              <SentRequestCard key={p.id} partner={p} token={token} onCancelled={onChanged} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function UserPlusIcon() {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke="currentColor" style="stroke-width: 1.5">
      <path strokeLinecap="round" strokeLinejoin="round" d="M18 7.5v3m0 0v3m0-3h3m-3 0h-3m-2.25-4.125a3.375 3.375 0 1 1-6.75 0 3.375 3.375 0 0 1 6.75 0ZM3 19.235v-.11a6.375 6.375 0 0 1 12.75 0v.109A12.318 12.318 0 0 1 9.374 21c-2.331 0-4.512-.645-6.374-1.766Z" />
    </svg>
  )
}


function InviteButton({ token, onInvited }: { token: string; onInvited: () => void }) {
  const [email, setEmail] = useState('');
  const [viewData, setViewData] = useState(true);
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
      await api.invitePartner(token, email, { view_data: viewData });
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
      <Button className="btn-primary btn-sm" onClick={open} icon={<UserPlusIcon/>}>
        Invite partner
      </Button>
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
              <input type="checkbox" checked={viewData} onChange={(e) => setViewData((e.target as HTMLInputElement).checked)} />
              Can view data
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
  const { userId } = useAuth();
  const e2ee = useE2EE();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [e2eePass, setE2EEPass] = useState('');

  const needsE2EE = partner.permissions.view_data;
  const { wrappingKey } = useAuth();

  async function accept(ev: Event) {
    ev.preventDefault();
    setLoading(true);
    setError(null);
    try {
      let encryptedE2EEKey: string | undefined;
      if (needsE2EE && userId && wrappingKey) {
        // Derive the monitored user's E2EE key and store locally
        const e2eeKey = await deriveKey(e2eePass, partner.partner_user_id, true);
        const rawE2EE = new Uint8Array(await crypto.subtle.exportKey('raw', e2eeKey));
        await e2ee.setKeyFromBytes(rawE2EE.buffer, partner.partner_user_id);
        // Encrypt it with own wrapping key for server storage
        const encrypted = await encryptData(wrappingKey, rawE2EE);
        encryptedE2EEKey = encrypted.toBase64();
      }
      await api.acceptPartner(token, partner.id, encryptedE2EEKey);
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
        {partner.permissions.view_data
          ? ' with access to your encrypted activity data.'
          : '.'}
      </p>
      <form onSubmit={accept}>
        {needsE2EE && (
          <>
            <div class="field" style="margin-top:0.75rem">
              <label for={`e2ee-${partner.id}`}>Their E2EE password</label>
              <input
                id={`e2ee-${partner.id}`}
                type="password"
                value={e2eePass}
                onInput={(e) => setE2EEPass((e.target as HTMLInputElement).value)}
                placeholder="Shared encryption password"
                required
              />
            </div>
          </>
        )}
        {error && <p class="form-error">{error}</p>}
        <button class="btn btn-primary btn-sm" type="submit" disabled={loading} style="margin-top:0.5rem">
          {loading ? 'Accepting…' : 'Accept invite'}
        </button>
      </form>
    </div>
  );
}

function PartnerDevicesSection({ partner, token }: { partner: Partner; token: string }) {
  const { route } = useLocation();
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
          onClick={() => route(`/logs?user=${partner.partner_user_id}`)}
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
                  onClick={() => route(`/logs?user=${partner.partner_user_id}&device_id=${d.id}`)}
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
  const { route } = useLocation();
  const online = device.status === 'online';
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
      await api.patchDevice(token, device.id, {
        name,
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
          onClick={() => route(`/logs?device_id=${device.id}`)}
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

function SentRequestCard({
  partner,
  token,
  onCancelled,
}: {
  partner: Partner;
  token: string;
  onCancelled: () => void;
}) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function cancel() {
    setLoading(true);
    setError(null);
    try {
      await api.deletePartner(token, partner.id);
      onCancelled();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to cancel request');
      setLoading(false);
    }
  }

  return (
    <div class="card">
      <div class="card-header">
        <span class="card-name">{partner.partner_email}</span>
        <span class="badge badge-yellow">Pending</span>
      </div>
      <dl class="card-meta">
        <dt>Sent</dt>
        <dd>{new Date(partner.created_at).toLocaleDateString()}</dd>
      </dl>
      {error && <p class="form-error">{error}</p>}
      <div class="card-actions">
        <button class="btn btn-danger btn-sm" type="button" onClick={cancel} disabled={loading}>
          {loading ? 'Cancelling…' : 'Cancel request'}
        </button>
      </div>
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
          <dt>Can view data</dt>
          <dd>{partner.permissions.view_data ? 'Yes' : 'No'}</dd>
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
