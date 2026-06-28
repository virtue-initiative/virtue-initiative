import { useMemo, useRef, useState } from 'preact/hooks';
import { useLocation } from 'preact-iso';
import {
  Device,
  WatchingPartner,
  WatcherPartner,
  useAPIContext,
  useDevices,
  usePartners,
} from '../../utils/api';
import { PageHeading } from '../../components/PageHeading';
import { PartnersIcon } from '../../components/icons';
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
  Field,
  Input,
  useToast,
} from '@virtueinitiative/shared-web';
import './style.css';

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

export function Partners() {
  const api = useAPIContext();
  const { devices } = useDevices();
  const { watchings: watching, watchers } = usePartners();
  const invitePartner = (email: string) =>
    api ? api.invitePartner(email) : Promise.reject(new Error('Not signed in'));
  const removeWatching = (id: string) =>
    api ? api.stopWatching(id) : Promise.reject(new Error('Not signed in'));
  const removeWatcher = (id: string) =>
    api ? api.removeWatcher(id) : Promise.reject(new Error('Not signed in'));

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
    () => watching.filter((partner) => partner.status === 'accepted'),
    [watching],
  );
  const pendingWatching = useMemo(
    () => watching.filter((partner) => partner.status === 'pending'),
    [watching],
  );
  const acceptedWatchers = useMemo(
    () => watchers.filter((partner) => partner.status === 'accepted'),
    [watchers],
  );
  const pendingWatchers = useMemo(
    () => watchers.filter((partner) => partner.status === 'pending'),
    [watchers],
  );

  return (
    <div class="dashboard">
      <PageHeading icon={<PartnersIcon />}>Partners</PageHeading>
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
    </div>
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
            partner.status === 'pending' ? (
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
                  'digest_cadence' in partner
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

function InviteButton({ onInvitePartner }: { onInvitePartner: (email: string) => Promise<void> }) {
  const [email, setEmail] = useState('');
  const [loading, setLoading] = useState(false);
  const { push: pushToast } = useToast();
  const dialogRef = useRef<HTMLDialogElement>(null);

  function open() {
    setEmail('');
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
      pushToast(err instanceof Error ? err.message : 'Failed to send invite', 'error');
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <Button variant="primary" type="button" onClick={open} style={{ gap: '0.4rem' }}>
        <UserPlusIcon /> Invite partner
      </Button>
      <Dialog dialogRef={dialogRef}>
        <DialogHeader>Invite a partner</DialogHeader>
        <p class="invite-desc">
          Your partner can <b>view any screenshots and activity logs </b>
          uploaded <b>after</b> you add them as a partner and they set up their account.
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
              {loading ? 'Sending…' : 'Send invite'}
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
  const [action, setAction] = useState<'remove' | null>(null);
  const { push: pushToast } = useToast();
  const confirmRef = useRef<HTMLDialogElement>(null);
  const partnerLabel = partner.user.name ?? partner.user.email;
  const partnerEmailTooltip = partner.user.name ? undefined : partner.user.email;
  const partnerName = partnerLabel;

  async function removeConfirmed() {
    setAction('remove');
    try {
      await ('digest_cadence' in partner
        ? onRemoveWatching(partner.id)
        : onRemoveWatcher(partner.id));
    } catch (err) {
      pushToast(err instanceof Error ? err.message : 'Failed to remove request', 'error');
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
          {action === 'remove' ? 'Removing…' : 'Remove'}
        </Button>
      </CardActions>
      <Dialog dialogRef={confirmRef}>
        <DialogHeader>Remove {partnerName}?</DialogHeader>
        <p class="invite-desc">This will cancel the pending partner relationship.</p>
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
            {action === 'remove' ? 'Removing…' : 'Remove partner'}
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
  const isWatching = 'digest_cadence' in partner;
  const [action, setAction] = useState<'remove' | null>(null);
  const { push: pushToast } = useToast();
  const confirmRef = useRef<HTMLDialogElement>(null);
  const partnerLabel = partner.user.name ?? partner.user.email;
  const partnerEmailTooltip = partner.user.name ? undefined : partner.user.email;
  const partnerName = partnerLabel;

  async function removeConfirmed() {
    setAction('remove');
    try {
      await ('digest_cadence' in partner
        ? onRemoveWatching(partner.id)
        : onRemoveWatcher(partner.id));
    } catch (err) {
      pushToast(err instanceof Error ? err.message : 'Failed to remove partner', 'error');
      setAction(null);
    }
  }

  return (
    <Card class={isWatching ? undefined : 'partner-card-compact'}>
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
              onClick={() => route(`/logs/${partner.user.id}?device_id=${device.id}`)}
              title={device.name}
            >
              <span
                class={`partner-device-status ${device.status === 'online' ? 'partner-device-status-online' : 'partner-device-status-offline'}`}
              />
              <span>{device.name}</span>
            </button>
          ))}
          {devices.length > 4 && (
            <p class="partner-device-more">+{devices.length - 4} more devices</p>
          )}
        </div>
      )}
      <CardActions>
        {isWatching && partner.user.id && (
          <Button variant="ghost" type="button" onClick={() => route(`/logs/${partner.user.id}`)}>
            View logs
          </Button>
        )}
        <Button
          variant="danger"
          type="button"
          onClick={() => confirmRef.current?.showModal()}
          disabled={action !== null}
        >
          {action === 'remove' ? 'Removing…' : 'Remove'}
        </Button>
      </CardActions>
      <Dialog dialogRef={confirmRef}>
        <DialogHeader>Remove {partnerName}?</DialogHeader>
        <p class="invite-desc">
          This will remove your partner relationship with this person. The partner will be notified.
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
            {action === 'remove' ? 'Removing…' : 'Remove partner'}
          </Button>
        </DialogActions>
      </Dialog>
    </Card>
  );
}
