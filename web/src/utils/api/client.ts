import {
  api,
  Batch,
  Device,
  PartnerRelationships,
  User,
  WatcherPartner,
  WatchingPartner,
} from './api';
import { decryptAndFlattenBatch } from './batch-materializer';
import { unwrapBatchKey } from './crypto';
import {
  clearDataCache,
  deleteDecryptedEventsForDevice,
  getUnmaterializedBatches,
  loadCachedDataFeed,
  mergeDataPageIntoCache,
  pruneCachedDataFeedDevices,
  pruneDecryptedEventsBefore,
  queryDecryptedEvents,
  removeDeviceFromCachedDataFeed,
  writeMaterializedEvents,
} from './data-cache';
import { FeedLog, getLogImage } from '../../pages/Logs/shared';
import { decodeWebpDimensions } from '../webp-dimensions';
import { Session } from './session';

const SYNC_PAGE_SIZE = 250;
const DECRYPT_CONCURRENCY = 5;
const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;

export interface UserSettings {
  email?: string;
  name?: string;
  email_frequency?: User['email_frequency'];
  email_digest_minutes_utc?: User['email_digest_minutes_utc'];
  pub_key?: string;
  priv_key?: string;
}

export interface UpdateSettingsResult {
  email_verification_required?: boolean;
  pending_email?: string;
}

export interface LogQuery {
  userId: string;
  deviceId?: string;
  startTime?: number;
  endTime?: number;
}

export interface LogQueryResult {
  logs: FeedLog[];
  complete: boolean;
}

type Subscriber<T> = (value: T) => void;

function notify<T>(set: Set<Subscriber<T>>, value: T) {
  for (const cb of set) {
    try {
      cb(value);
    } catch (err) {
      console.error('[api-client] subscriber threw', err);
    }
  }
}

export class APIClient {
  readonly session: Session;
  readonly userId: string;

  private userCache: User | null = null;
  private userSubscribers = new Set<Subscriber<User | null>>();
  private userFetchInFlight: Promise<User | null> | null = null;

  private watchersCache: WatcherPartner[] | null = null;
  private watchersSubscribers = new Set<Subscriber<WatcherPartner[]>>();
  private watchingsCache: WatchingPartner[] | null = null;
  private watchingsSubscribers = new Set<Subscriber<WatchingPartner[]>>();
  private partnersFetchInFlight: Promise<PartnerRelationships | null> | null = null;

  private devicesCache: Device[] | null = null;
  private devicesSubscribers = new Set<Subscriber<Device[]>>();
  private devicesFetchInFlight: Promise<Device[] | null> | null = null;

  private logoutSubscribers = new Set<() => void>();
  private loggedOut = false;

  constructor(session: Session) {
    this.session = session;
    this.userId = session.userId;
    session.onTokenRefreshFailed(() => {
      this.fireLogoutOnce();
    });
  }

  // ── User ────────────────────────────────────────────────────────────────
  getUser(): User | null {
    if (this.userCache === null) {
      void this.fetchUser();
    }
    return this.userCache;
  }

  subscribeUser(cb: Subscriber<User | null>): { user: User | null; unsubscribe: () => void } {
    if (this.userCache === null) {
      void this.fetchUser();
    }
    this.userSubscribers.add(cb);
    return {
      user: this.userCache,
      unsubscribe: () => this.userSubscribers.delete(cb),
    };
  }

  async updateSettings(settings: UserSettings): Promise<UpdateSettingsResult> {
    const result = await api.updateUser(this.session.token, settings);
    await this.fetchUser(true);
    return {
      email_verification_required: result.email_verification_required,
      pending_email: result.pending_email,
    };
  }

  async deleteUser(confirmEmail: string): Promise<void> {
    await api.deleteUser(this.session.token, confirmEmail);
    this.userCache = null;
    notify(this.userSubscribers, null);
  }

  private async fetchUser(force = false): Promise<User | null> {
    if (this.userFetchInFlight && !force) return this.userFetchInFlight;
    const p = (async () => {
      try {
        const user = await api.getUser(this.session.token);
        this.userCache = user;
        notify(this.userSubscribers, user);
        return user;
      } catch (err) {
        console.warn('[api-client] failed to fetch user', err);
        return null;
      } finally {
        this.userFetchInFlight = null;
      }
    })();
    this.userFetchInFlight = p;
    return p;
  }

  // ── Auth ─────────────────────────────────────────────────────────────────
  async requestResetPassword(email: string): Promise<void> {
    await api.requestPasswordReset(email);
  }

  async resetPassword(
    token: string,
    payload: {
      password_auth: string;
      password_salt: string;
      pub_key?: string;
      priv_key?: string;
    },
  ): Promise<void> {
    await api.resetPassword(token, payload);
  }

  async logout(): Promise<void> {
    await this.session.logout();
    await clearDataCache().catch(() => {});
    this.fireLogoutOnce();
  }

  // ── Partners ─────────────────────────────────────────────────────────────
  listWatchers(): WatcherPartner[] {
    if (this.watchersCache === null) {
      void this.fetchPartners();
    }
    return this.watchersCache ?? [];
  }

  subscribeWatchers(cb: Subscriber<WatcherPartner[]>): {
    watchers: WatcherPartner[];
    loaded: boolean;
    unsubscribe: () => void;
  } {
    if (this.watchersCache === null) {
      void this.fetchPartners();
    }
    this.watchersSubscribers.add(cb);
    return {
      watchers: this.watchersCache ?? [],
      loaded: this.watchersCache !== null,
      unsubscribe: () => this.watchersSubscribers.delete(cb),
    };
  }

  listWatchings(): WatchingPartner[] {
    if (this.watchingsCache === null) {
      void this.fetchPartners();
    }
    return this.watchingsCache ?? [];
  }

  subscribeWatchings(cb: Subscriber<WatchingPartner[]>): {
    watchings: WatchingPartner[];
    loaded: boolean;
    unsubscribe: () => void;
  } {
    if (this.watchingsCache === null) {
      void this.fetchPartners();
    }
    this.watchingsSubscribers.add(cb);
    return {
      watchings: this.watchingsCache ?? [],
      loaded: this.watchingsCache !== null,
      unsubscribe: () => this.watchingsSubscribers.delete(cb),
    };
  }

  async invitePartner(email: string): Promise<void> {
    await api.invitePartner(this.session.token, email);
    await this.fetchPartners(true);
  }

  async acceptInvite(inviteToken: string): Promise<void> {
    await api.acceptPartnerInvite(this.session.token, inviteToken);
    await this.fetchPartners(true);
    await this.fetchDevices(true);
  }

  async removeWatcher(id: string): Promise<void> {
    await api.deleteWatcher(this.session.token, id);
    await this.fetchPartners(true);
    await this.fetchDevices(true);
  }

  async stopWatching(id: string): Promise<void> {
    await api.deleteWatching(this.session.token, id);
    await this.fetchPartners(true);
    await this.fetchDevices(true);
  }

  private async fetchPartners(force = false): Promise<PartnerRelationships | null> {
    if (this.partnersFetchInFlight && !force) return this.partnersFetchInFlight;
    const p = (async () => {
      try {
        const result = await api.getPartners(this.session.token);
        this.watchersCache = result.watchers;
        this.watchingsCache = result.watching;
        notify(this.watchersSubscribers, result.watchers);
        notify(this.watchingsSubscribers, result.watching);
        return result;
      } catch (err) {
        console.warn('[api-client] failed to fetch partners', err);
        return null;
      } finally {
        this.partnersFetchInFlight = null;
      }
    })();
    this.partnersFetchInFlight = p;
    return p;
  }

  // ── Devices ─────────────────────────────────────────────────────────────
  listDevices(): Device[] {
    if (this.devicesCache === null) {
      void this.fetchDevices();
    }
    return this.devicesCache ?? [];
  }

  subscribeDevices(cb: Subscriber<Device[]>): {
    devices: Device[];
    loaded: boolean;
    unsubscribe: () => void;
  } {
    if (this.devicesCache === null) {
      void this.fetchDevices();
    }
    this.devicesSubscribers.add(cb);
    return {
      devices: this.devicesCache ?? [],
      loaded: this.devicesCache !== null,
      unsubscribe: () => this.devicesSubscribers.delete(cb),
    };
  }

  async updateDevice(id: string, patch: { name?: string }): Promise<void> {
    await api.patchDevice(this.session.token, id, patch);
    await this.fetchDevices(true);
  }

  async removeDevice(id: string): Promise<void> {
    await api.deleteDevice(this.session.token, id);
    await removeDeviceFromCachedDataFeed(this.userId, this.userId, id).catch((err) =>
      console.warn('[api-client] failed to remove deleted device from cache', err),
    );
    await deleteDecryptedEventsForDevice(this.userId, id).catch((err) =>
      console.warn('[api-client] failed to wipe decrypted events for device', err),
    );
    await this.fetchDevices(true);
  }

  private async fetchDevices(force = false): Promise<Device[] | null> {
    if (this.devicesFetchInFlight && !force) return this.devicesFetchInFlight;
    const p = (async () => {
      try {
        const devices = await api.getDevices(this.session.token);
        this.devicesCache = devices;
        notify(this.devicesSubscribers, devices);
        return devices;
      } catch (err) {
        console.warn('[api-client] failed to fetch devices', err);
        return null;
      } finally {
        this.devicesFetchInFlight = null;
      }
    })();
    this.devicesFetchInFlight = p;
    return p;
  }

  // ── Logs ────────────────────────────────────────────────────────────────
  queryLogs(query: LogQuery, cb?: (result: LogQueryResult) => void): LogQueryResult {
    const initial = queryLogsFromIDB(this.userId, query);
    let result: LogQueryResult = { logs: [], complete: false };
    initial
      .then((logs) => {
        result = { logs, complete: false };
        if (cb) cb(result);
      })
      .catch((err) => console.warn('[api-client] initial log query failed', err));

    if (cb) {
      void this.refreshLogs(query, cb);
    }

    return result;
  }

  private async refreshLogs(query: LogQuery, cb: (result: LogQueryResult) => void): Promise<void> {
    const { userId: targetUserId, deviceId, startTime, endTime } = query;

    try {
      await pruneDecryptedEventsBefore(this.userId, Date.now() - THIRTY_DAYS_MS).catch(() => {});

      const devices: Device[] = this.devicesCache ?? (await this.fetchDevices()) ?? [];
      const ownedIds = devices
        .filter((d) => d.owner === targetUserId)
        .map((d) => d.id)
        .sort();

      const initialFeed = await loadCachedDataFeed(this.userId, targetUserId);
      let since = initialFeed.since;

      while (true) {
        const page = await api.getData(this.session.token, {
          user: targetUserId === this.userId ? undefined : targetUserId,
          since,
          limit: SYNC_PAGE_SIZE,
        });

        if (page.batches.length === 0 && page.logs.length === 0) {
          break;
        }

        const updated = await mergeDataPageIntoCache(this.userId, targetUserId, page);
        since = updated.since;

        if (page.next_since === undefined) {
          break;
        }
      }

      let cachedFeed = await loadCachedDataFeed(this.userId, targetUserId);
      cachedFeed = await pruneCachedDataFeedDevices(this.userId, targetUserId, ownedIds);

      const inRangeBatches = cachedFeed.batches.filter((batch) => {
        if (deviceId && batch.device_id !== deviceId) return false;
        if (startTime !== undefined && endTime !== undefined) {
          return batch.start_time <= endTime && batch.end_time >= startTime;
        }
        return true;
      });

      const cutoff = Date.now() - THIRTY_DAYS_MS;
      const unmaterialized = getUnmaterializedBatches(cachedFeed, inRangeBatches, cutoff);

      const cachedLogs = await queryLogsFromIDB(this.userId, query);
      cb({
        logs: cachedLogs,
        complete: unmaterialized.length === 0 || !this.session.privateKey,
      });

      if (!this.session.privateKey || unmaterialized.length === 0) return;

      const queue = [...unmaterialized].sort((a, b) => b.created_at - a.created_at);
      let processed = 0;
      const total = queue.length;

      const worker = async () => {
        while (queue.length > 0) {
          const batch = queue.shift()!;
          try {
            const events = await decryptAndFlattenBatch(
              batch,
              (encryptedKey) =>
                unwrapBatchKey(this.session.privateKey!, Uint8Array.fromBase64(encryptedKey)),
              '0'.repeat(64),
            );
            await writeMaterializedEvents(this.userId, targetUserId, batch.id, events);
          } catch (err) {
            console.warn('[api-client] failed to materialize batch', batch.id, err);
          }
          processed++;
          const updatedLogs = await queryLogsFromIDB(this.userId, query);
          cb({ logs: updatedLogs, complete: processed === total });
        }
      };

      await Promise.all(Array.from({ length: Math.min(DECRYPT_CONCURRENCY, total) }, worker));
    } catch (err) {
      console.warn('[api-client] log refresh failed', err);
    }
  }

  // ── Lifecycle ───────────────────────────────────────────────────────────
  onLogout(cb: () => void): () => void {
    this.logoutSubscribers.add(cb);
    return () => this.logoutSubscribers.delete(cb);
  }

  private fireLogoutOnce() {
    if (this.loggedOut) return;
    this.loggedOut = true;
    for (const cb of this.logoutSubscribers) {
      try {
        cb();
      } catch (err) {
        console.error('[api-client] logout subscriber threw', err);
      }
    }
  }
}

async function queryLogsFromIDB(viewerId: string, query: LogQuery): Promise<FeedLog[]> {
  const cachedFeed = await loadCachedDataFeed(viewerId, query.userId);

  const feedDeviceIds = new Set([
    ...cachedFeed.batches.map((b) => b.device_id),
    ...cachedFeed.logs.map((l) => l.device_id),
  ]);

  const events = await queryDecryptedEvents(viewerId, {
    deviceId: query.deviceId,
    allowedDeviceIds: query.deviceId ? undefined : [...feedDeviceIds],
    startTs: query.startTime,
    endTs: query.endTime,
  });

  const directLogs: FeedLog[] = cachedFeed.logs
    .filter((log) => {
      if (query.deviceId && log.device_id !== query.deviceId) return false;
      if (query.startTime !== undefined && query.endTime !== undefined) {
        return log.ts >= query.startTime && log.ts <= query.endTime;
      }
      return true;
    })
    .map(toDirectLogEntry);

  return [...events, ...directLogs].sort((a, b) => b.ts - a.ts);
}

function toDirectLogEntry(entry: {
  id: string;
  device_id: string;
  ts: number;
  type: string;
  data: Record<string, unknown>;
  created_at: number;
  risk?: number;
}): FeedLog {
  const image = getLogImage(entry);
  let image_w: number | undefined;
  let image_h: number | undefined;
  if (image) {
    const dims = decodeWebpDimensions(image);
    if (dims) {
      image_w = dims.width;
      image_h = dims.height;
    }
  }
  return {
    ...entry,
    batch_status: 'unknown' as const,
    source: 'log' as const,
    image_w,
    image_h,
  };
}

export type { Batch };
