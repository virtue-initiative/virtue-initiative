import { listAnnouncements } from './db';
import type { Announcement, DeviceAnnouncement } from '../../../shared-web/types';

// API-068: the table may be cached for up to 60 seconds. It's read on every
// device settings response, so without this each would cost one more query.
const CACHE_TTL_MS = 60_000;
let cached: { announcements: Announcement[]; expiresAt: number } | null = null;

async function loadAnnouncements(db: D1Database, now: number) {
  if (cached && cached.expiresAt > now) {
    return cached.announcements;
  }
  const announcements = await listAnnouncements(db);
  cached = { announcements, expiresAt: now + CACHE_TTL_MS };
  return announcements;
}

// Called after an admin route changes an announcement, so this isolate sees it at once.
export function clearAnnouncementCache() {
  cached = null;
}

// API-068: dot-separated whole numbers, ignoring everything from the first
// character that is neither a digit nor a dot. Null when there is no leading
// number, which counts as lower than every version.
export function parseVersion(version: string | null | undefined): number[] | null {
  const prefix = /^[\d.]*/.exec(version?.trim() ?? '')![0];
  const parts = prefix.split('.').filter((part) => part !== '');
  if (parts.length === 0 || !/^\d/.test(prefix)) return null;
  return parts.map((part) => Number.parseInt(part, 10));
}

// True when `version` is lower than `bound`, with missing parts counting as 0.
export function isVersionBelow(version: string | null | undefined, bound: string) {
  const parsedBound = parseVersion(bound);
  const parsed = parseVersion(version);
  if (!parsedBound) return false;
  if (!parsed) return true;
  for (let i = 0; i < Math.max(parsed.length, parsedBound.length); i++) {
    const a = parsed[i] ?? 0;
    const b = parsedBound[i] ?? 0;
    if (a !== b) return a < b;
  }
  return false;
}

// The newest announcement that applies to a device, for DeviceSettings (API-068).
export async function announcementForDevice(
  db: D1Database,
  device: { platform: string; version: string | null | undefined },
  now = Date.now(),
): Promise<DeviceAnnouncement | null> {
  const announcements = await loadAnnouncements(db, now);
  const match = announcements.find(
    (a) =>
      (a.platforms === null || a.platforms.includes(device.platform)) &&
      (a.below_version === null || isVersionBelow(device.version, a.below_version)),
  );
  return match ? { message: match.message, url: match.url, level: match.level } : null;
}
