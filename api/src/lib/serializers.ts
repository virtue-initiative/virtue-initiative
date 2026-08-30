import { encodeBase64 } from './encoding';
import type { findUserById, listIncomingPartners, listOwnedPartners } from './db';

type UserRow = NonNullable<Awaited<ReturnType<typeof findUserById>>>;
type IncomingPartnerRow = Awaited<ReturnType<typeof listIncomingPartners>>[number];
type OwnedPartnerRow = Awaited<ReturnType<typeof listOwnedPartners>>[number];

export function serializeUser(user: UserRow) {
  return {
    id: user.id,
    email: user.email,
    email_verified: user.email_verified === 1,
    email_bounced_at: user.email_bounced_at,
    settings: user.settings,
    ...(user.name ? { name: user.name } : {}),
    ...(user.pub_key ? { pub_key: encodeBase64(user.pub_key) } : {}),
    ...(user.encrypted_priv_key
      ? { encrypted_priv_key: encodeBase64(user.encrypted_priv_key) }
      : {}),
  };
}

export function serializeWatching(incoming: IncomingPartnerRow[]) {
  return incoming.map((partner) => ({
    id: partner.id,
    user: {
      id: partner.watching_user_id,
      email: partner.watching_user_email,
      ...(partner.watching_user_name ? { name: partner.watching_user_name } : {}),
    },
    status: partner.status,
    created_at: partner.created_at,
  }));
}

export function serializeWatchers(owned: OwnedPartnerRow[]) {
  return owned.map((partner) => ({
    id: partner.id,
    user: {
      ...(partner.watcher_id ? { id: partner.watcher_id } : {}),
      email: partner.watcher_email,
      ...(partner.watcher_name ? { name: partner.watcher_name } : {}),
    },
    status: partner.status,
    created_at: partner.created_at,
  }));
}
