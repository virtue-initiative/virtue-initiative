import {
  api,
  Batch,
  Device,
  PartnerRelationships,
  Updates,
  User,
  WatcherPartner,
  WatchingPartner,
} from './api';
import { FeedLog } from '../../pages/Logs/types';
import { Session } from './session';
import { cacheClient } from '../cache/client';

export interface UserSettings {
  email?: string;
  name?: string;
  settings?: { email_frequency?: User['settings']['email_frequency']; timezone?: string };
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
  /** Number of batch blocks decrypted so far in the in-flight sync. */
  processed: number;
  /** Total batch blocks to decrypt in the in-flight sync (0 if unknown). */
  total: number;
}

// Merge an incremental delta into an existing log set: dedupe by id (incoming wins) and keep
// the newest-first (ts desc) ordering that sqlQueryEvents produces.
function mergeLogs(existing: FeedLog[], incoming: FeedLog[]): FeedLog[] {
  const byId = new Map(existing.map((log) => [log.id, log]));
  for (const log of incoming) byId.set(log.id, log);
  return [...byId.values()].sort((a, b) => b.ts - a.ts);
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

  private unsubscribeUpdates: (() => void) | null = null;

  constructor(session: Session) {
    this.session = session;
    this.userId = session.userId;

    const updates = session.updates;
    this.userCache = session.user ?? null;
    this.devicesCache = updates?.devices ?? null;
    this.watchersCache = updates?.partners.watchers ?? null;
    this.watchingsCache = updates?.partners.watching ?? null;

    cacheClient?.setSession(session.userId, session.privateKey ?? null);
    this.unsubscribeUpdates =
      cacheClient?.subscribeUpdates((next) => this.applyUpdates(next)) ?? null;

    session.onTokenRefreshFailed(() => {
      this.fireLogoutOnce();
    });
  }

  private applyUpdates(updates: Updates): void {
    this.userCache = updates.user;
    notify(this.userSubscribers, updates.user);
    this.devicesCache = updates.devices;
    notify(this.devicesSubscribers, updates.devices);
    this.watchersCache = updates.partners.watchers;
    notify(this.watchersSubscribers, updates.partners.watchers);
    this.watchingsCache = updates.partners.watching;
    notify(this.watchingsSubscribers, updates.partners.watching);
  }

  private async refreshAfterMutation(fallback: () => Promise<unknown>): Promise<void> {
    if (cacheClient) {
      await cacheClient.refetchUpdates().catch((err) => {
        console.warn('[api-client] worker refetchUpdates failed', err);
      });
      return;
    }
    await fallback();
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
    const result = await api.updateUser(settings);
    await this.refreshAfterMutation(() => this.fetchUser(true));
    return {
      email_verification_required: result.email_verification_required,
      pending_email: result.pending_email,
    };
  }

  async deleteUser(confirmEmail: string): Promise<void> {
    await api.deleteUser(confirmEmail);
    this.userCache = null;
    notify(this.userSubscribers, null);
  }

  private async fetchUser(force = false): Promise<User | null> {
    if (this.userFetchInFlight && !force) return this.userFetchInFlight;
    const p = (async () => {
      try {
        const user = await api.getUser();
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
  async verifyEmailChange(token: string): Promise<void> {
    await api.verifyEmail(token);
    const user = await api.getUser();
    this.userCache = user;
    notify(this.userSubscribers, user);
  }

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
    await cacheClient?.clearCache().catch(() => {});
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
    await api.invitePartner(email);
    await this.refreshAfterMutation(() => this.fetchPartners(true));
  }

  async acceptInvite(inviteToken: string): Promise<void> {
    await api.acceptPartnerInvite(inviteToken);
    await this.refreshAfterMutation(() =>
      Promise.all([this.fetchPartners(true), this.fetchDevices(true)]),
    );
  }

  async removeWatcher(id: string): Promise<void> {
    await api.deleteWatcher(id);
    await this.refreshAfterMutation(() =>
      Promise.all([this.fetchPartners(true), this.fetchDevices(true)]),
    );
  }

  async stopWatching(id: string): Promise<void> {
    await api.deleteWatching(id);
    await this.refreshAfterMutation(() =>
      Promise.all([this.fetchPartners(true), this.fetchDevices(true)]),
    );
  }

  private async fetchPartners(force = false): Promise<PartnerRelationships | null> {
    if (this.partnersFetchInFlight && !force) return this.partnersFetchInFlight;
    const p = (async () => {
      try {
        const result = await api.getPartners();
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
    await api.patchDevice(id, patch);
    await this.refreshAfterMutation(() => this.fetchDevices(true));
  }

  async removeDevice(id: string): Promise<void> {
    await api.deleteDevice(id);
    cacheClient
      ?.deleteDeviceData(this.userId, id)
      .catch((err) => console.warn('[api-client] failed to wipe device data from cache', err));
    await this.refreshAfterMutation(() => this.fetchDevices(true));
  }

  private async fetchDevices(force = false): Promise<Device[] | null> {
    if (this.devicesFetchInFlight && !force) return this.devicesFetchInFlight;
    const p = (async () => {
      try {
        const devices = await api.getDevices();
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
    if (cb) {
      // Updates come in three flavours: a `replace` snapshot (cached fast-path, then the
      // final complete result), an `append` delta (logs decrypted in the last interval), and
      // counts-only progress ticks (no logs). Retain the running set so every update — even
      // counts-only — can re-emit a complete LogQueryResult.
      let lastLogs: FeedLog[] = [];
      cacheClient?.cacheQuery(
        {
          userId: query.userId,
          deviceId: query.deviceId,
          startTime: query.startTime,
          endTime: query.endTime,
        },
        (update) => {
          if (update.logs) {
            lastLogs = update.replace ? update.logs : mergeLogs(lastLogs, update.logs);
          }
          cb({
            logs: lastLogs,
            complete: update.done,
            processed: update.processed,
            total: update.total,
          });
        },
      );
    }
    return { logs: [], complete: false, processed: 0, total: 0 };
  }

  // ── Lifecycle ───────────────────────────────────────────────────────────
  onLogout(cb: () => void): () => void {
    this.logoutSubscribers.add(cb);
    return () => this.logoutSubscribers.delete(cb);
  }

  private fireLogoutOnce() {
    if (this.loggedOut) return;
    this.loggedOut = true;
    this.unsubscribeUpdates?.();
    for (const cb of this.logoutSubscribers) {
      try {
        cb();
      } catch (err) {
        console.error('[api-client] logout subscriber threw', err);
      }
    }
  }
}

export type { Batch };
