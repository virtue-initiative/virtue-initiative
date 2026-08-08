import { encodeBase64 } from './encoding';
import {
  findUserById,
  getHashState,
  listDevicesForOwners,
  listIncomingPartners,
  listOwnedPartners,
  listVisibleOwnerIds,
} from './db';
import { generateToken } from './jwt';
import { DEFAULT_EMAIL_FREQUENCY, emailFrequencies } from './email-domain';
import { Env } from '../types/bindings';
import type { Device, PartnerRelationships, User } from '../../../shared-web/types';

export const ONLINE_WINDOW_MS = 2 * 60 * 60 * 1000;

export async function buildUserView(db: D1Database, userId: string): Promise<User | null> {
  const user = await findUserById(db, userId);
  if (!user) return null;
  return {
    id: user.id,
    email: user.email,
    email_verified: user.email_verified === 1,
    email_bounced_at: user.email_bounced_at,
    settings: {
      email_frequency: toPublicNotificationCadence(user.settings.email_frequency),
      timezone: user.settings.timezone,
    },
    ...(user.name ? { name: user.name } : {}),
    ...(user.pub_key ? { pub_key: encodeBase64(user.pub_key) } : {}),
    ...(user.priv_key ? { priv_key: encodeBase64(user.priv_key) } : {}),
  };
}

export async function buildDeviceViews(env: Env, requesterId: string): Promise<Device[]> {
  const ownerIds = await listVisibleOwnerIds(env.DB, requesterId);
  const rows = await listDevicesForOwners(env.DB, ownerIds);
  const hashServerUrl = env.HASH_SERVER_URL?.trim() || null;
  const hashInfo = new Map<string, { count: number; hashed_at: number | null }>();

  if (hashServerUrl?.endsWith('/api')) {
    // Hack: when the hash server is this API itself, skip the HTTP round-trip
    // and read the hash state directly from D1.
    await Promise.all(
      rows.map(async (device) => {
        const state = await getHashState(env.DB, device.id);
        if (state) {
          hashInfo.set(device.id, { count: state.count, hashed_at: state.hashed_at });
        }
      }),
    );
  } else if (hashServerUrl) {
    await Promise.all(
      rows.map(async (device) => {
        try {
          const token = await generateToken('server', device.id, env.JWT_PRIVATE_KEY, 60);
          const resp = await fetch(`${hashServerUrl}/hash/info`, {
            headers: { Authorization: `Bearer ${token}` },
          });
          if (resp.ok) {
            const info = (await resp.json()) as { count: number; hashed_at: number | null };
            hashInfo.set(device.id, info);
          }
        } catch {
          // fall back to D1 values for this device
        }
      }),
    );
  }

  return rows.map((device) => {
    const hi = hashInfo.get(device.id);
    return {
      id: device.id,
      owner: device.owner,
      name: device.name,
      platform: device.platform,
      last_upload_at: device.last_upload_at,
      last_hash_at: hi ? hi.hashed_at : device.last_hash_at,
      pending_count: hi ? hi.count : device.pending_count,
      status: device.deleted_at
        ? 'logged_out'
        : device.last_upload_at && Date.now() - device.last_upload_at < ONLINE_WINDOW_MS
          ? 'online'
          : 'offline',
    };
  });
}

function toPublicNotificationCadence(emailFrequency: string | null | undefined) {
  if (!emailFrequency || !(emailFrequencies as readonly string[]).includes(emailFrequency)) {
    return 'daily' as const;
  }

  return emailFrequency as (typeof emailFrequencies)[number];
}

function toPartnerStatus(status: string): 'pending' | 'accepted' {
  return status === 'accepted' ? 'accepted' : 'pending';
}

export async function buildPartnerRelationships(
  db: D1Database,
  userId: string,
): Promise<PartnerRelationships> {
  const [owned, incoming] = await Promise.all([
    listOwnedPartners(db, userId),
    listIncomingPartners(db, userId),
  ]);

  return {
    watching: incoming.map((partner) => ({
      id: partner.id,
      user: {
        id: partner.watching_user_id,
        email: partner.watching_user_email,
        ...(partner.watching_user_name ? { name: partner.watching_user_name } : {}),
      },
      status: toPartnerStatus(partner.status),
      digest_cadence: toPublicNotificationCadence(
        partner.settings.email_frequency ?? DEFAULT_EMAIL_FREQUENCY,
      ),
      created_at: partner.created_at,
    })),
    watchers: owned.map((partner) => ({
      id: partner.id,
      user: {
        ...(partner.watcher_id ? { id: partner.watcher_id } : {}),
        email: partner.watcher_email,
        ...(partner.watcher_name ? { name: partner.watcher_name } : {}),
      },
      status: toPartnerStatus(partner.status),
      created_at: partner.created_at,
    })),
  };
}
