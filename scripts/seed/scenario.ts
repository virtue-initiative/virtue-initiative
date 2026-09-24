// The sample people, devices and activity the seeder writes. Activity covers
// the last DAYS days up to "now", so both the daily and the weekly report
// have something to show right after seeding.
import type { SeedUser } from './accounts';
import type { SeedEvent } from './batch';
import { readFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { rng, type Rng } from './util';

const IMAGE_DIR = join(dirname(fileURLToPath(import.meta.url)), 'images');
const imageCache = new Map<string, Uint8Array>();
function image(name: string): Uint8Array {
  let bytes = imageCache.get(name);
  if (!bytes) {
    bytes = new Uint8Array(readFileSync(join(IMAGE_DIR, `${name}.webp`)));
    imageCache.set(name, bytes);
  }
  return bytes;
}

export const DAYS = 8;

const bytes = (length: number, fill: number) => new Uint8Array(length).fill(fill);

// dev@dev.com keeps the id, salt and key pair the old single-user seed script
// used, so existing local databases and the client integration tests still work.
export const DEV: SeedUser = {
  idHex: '0123456789ab4def8123456789abcdef',
  email: 'dev@dev.com',
  name: 'Dev',
  salt: new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
  keyIkm: bytes(32, 0x42),
  keyNonce: new Uint8Array([17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28]),
};

export const SAM: SeedUser = {
  idHex: 'a0000000000040008000000000000002',
  email: 'sam@dev.com',
  name: 'Sam Rivera',
  salt: bytes(16, 0x02),
  keyIkm: bytes(32, 0x52),
  keyNonce: bytes(12, 0x12),
};

export const JORDAN: SeedUser = {
  idHex: 'a0000000000040008000000000000003',
  email: 'jordan@dev.com',
  name: 'Jordan Lee',
  salt: bytes(16, 0x03),
  keyIkm: bytes(32, 0x53),
  keyNonce: bytes(12, 0x13),
};

export const USERS = [DEV, SAM, JORDAN];

// Each watcher can decrypt the watched user's batches (access_keys) and sees
// them in the Logs and Report tabs.
export const PARTNERSHIPS: { idHex: string; watching: SeedUser; watcher: SeedUser }[] = [
  { idHex: 'b0000000000040008000000000000001', watching: SAM, watcher: DEV },
  { idHex: 'b0000000000040008000000000000002', watching: JORDAN, watcher: DEV },
];

type FormFactor = 'desktop' | 'phone';

export interface SeedDevice {
  idHex: string;
  owner: SeedUser;
  name: string;
  platform: 'linux' | 'windows' | 'macos' | 'android' | 'ios';
  form: FormFactor;
  // How flagged this person's week is. 0 = nothing concerning at all.
  riskiness: number;
  // Days ago (0 = today) the device uploaded nothing, e.g. left at home.
  quietDays: number[];
}

export const DEVICES: SeedDevice[] = [
  {
    idHex: 'd0000000000040008000000000000001',
    owner: DEV,
    name: 'Dev Laptop',
    platform: 'linux',
    form: 'desktop',
    riskiness: 0.15,
    quietDays: [],
  },
  {
    idHex: 'd0000000000040008000000000000002',
    owner: DEV,
    name: 'Dev Phone',
    platform: 'android',
    form: 'phone',
    riskiness: 0.15,
    quietDays: [3],
  },
  {
    idHex: 'd0000000000040008000000000000003',
    owner: SAM,
    name: 'Sam’s ThinkPad',
    platform: 'linux',
    form: 'desktop',
    riskiness: 1,
    quietDays: [],
  },
  {
    idHex: 'd0000000000040008000000000000004',
    owner: SAM,
    name: 'Gaming PC',
    platform: 'windows',
    form: 'desktop',
    riskiness: 1,
    quietDays: [2, 5],
  },
  {
    idHex: 'd0000000000040008000000000000005',
    owner: SAM,
    name: 'Pixel 8',
    platform: 'android',
    form: 'phone',
    riskiness: 1,
    quietDays: [],
  },
  {
    idHex: 'd0000000000040008000000000000006',
    owner: SAM,
    name: 'iPad',
    platform: 'ios',
    form: 'phone',
    riskiness: 1,
    // Not seen for the last couple of days, so it shows as offline.
    quietDays: [0, 1],
  },
  {
    idHex: 'd0000000000040008000000000000007',
    owner: JORDAN,
    name: 'Jordan’s MacBook',
    platform: 'macos',
    form: 'desktop',
    riskiness: 0,
    quietDays: [],
  },
];

const SAFE_IMAGES: Record<FormFactor, string[]> = {
  desktop: ['docs', 'code', 'chat', 'email', 'video', 'news', 'search'],
  phone: ['phone-home', 'phone-feed'],
};
const HIGH_IMAGE: Record<FormFactor, string> = { desktop: 'flagged', phone: 'flagged-phone' };
const MEDIUM_IMAGE: Record<FormFactor, string> = {
  desktop: 'questionable',
  phone: 'questionable-phone',
};

export interface SeedBatch {
  device: SeedDevice;
  events: SeedEvent[];
  startTime: number;
  endTime: number;
}

const MINUTE = 60_000;
const SCREENSHOT_INTERVAL = 4 * MINUTE;

function screenshot(ts: number, name: string, risk: number, r: Rng): SeedEvent {
  return {
    ts,
    risk,
    type: 'screenshot',
    data: {
      image: image(name),
      content_type: 'image/webp',
      skin_detection: risk > 0.3 ? Math.min(1, risk + r.range(0, 0.1)) : r.range(0, 0.2),
      nsfw_detection: risk,
    },
  };
}

// One or more usage sessions per device per day; each session becomes a batch,
// like a client uploading after a stretch of activity.
function sessionsFor(device: SeedDevice, dayStart: number, now: number, r: Rng) {
  const sessions: { start: number; end: number }[] = [];
  const starts =
    device.form === 'phone' ? [8, 12.5, 17.5, 21] : r.chance(0.5) ? [9, 14, 20] : [10, 19.5];
  for (const hour of starts) {
    if (r.chance(0.2)) continue;
    const start = dayStart + (hour + r.range(-0.5, 0.5)) * 60 * MINUTE;
    const length = (device.form === 'phone' ? r.int(15, 45) : r.int(40, 110)) * MINUTE;
    const end = Math.min(start + length, now - MINUTE);
    if (end - start > 5 * MINUTE) sessions.push({ start, end });
  }
  return sessions;
}

export function generateBatches(now: number): SeedBatch[] {
  const batches: SeedBatch[] = [];
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);

  for (let daysAgo = DAYS - 1; daysAgo >= 0; daysAgo--) {
    const dayStart = new Date(today);
    dayStart.setDate(today.getDate() - daysAgo);
    // Seed on the calendar date so re-running on the same day reproduces it.
    const dateSeed = dayStart.getFullYear() * 1000 + dayStart.getMonth() * 40 + dayStart.getDate();

    for (const device of DEVICES) {
      if (device.quietDays.includes(daysAgo)) continue;
      const r = rng(dateSeed * 31 + parseInt(device.idHex.slice(-4), 16));
      const sessions = sessionsFor(device, dayStart.getTime(), now, r);

      for (const [index, session] of sessions.entries()) {
        const events: SeedEvent[] = [];
        if (index === 0) {
          events.push({
            ts: session.start - MINUTE,
            type: 'system_login',
            data: { utc_ms: session.start - MINUTE },
          });
        }

        for (
          let ts = session.start;
          ts <= session.end;
          ts += SCREENSHOT_INTERVAL + r.int(-40, 40) * 1000
        ) {
          const roll = r.next();
          const risky = device.riskiness;
          if (roll < 0.035 * risky) {
            events.push(screenshot(ts, HIGH_IMAGE[device.form], r.range(0.72, 0.97), r));
          } else if (roll < 0.14 * risky) {
            events.push(screenshot(ts, MEDIUM_IMAGE[device.form], r.range(0.41, 0.68), r));
          } else if (roll < 0.2) {
            events.push({ ts, type: 'screenshot_skipped', data: { reason: 'static_screen' } });
          } else {
            events.push(
              screenshot(
                ts,
                r.pick(SAFE_IMAGES[device.form]),
                r.chance(0.6) ? 0 : r.range(0.01, 0.3),
                r,
              ),
            );
          }
        }

        // Tamper-style alerts, mostly on the higher-risk person's devices.
        const mid = session.start + (session.end - session.start) / 2;
        if (r.chance(0.12 * device.riskiness)) {
          events.push({ ts: mid, risk: 0.8, type: 'screenshot_missed' });
        }
        if (r.chance(0.08 * device.riskiness)) {
          events.push({
            ts: mid + MINUTE,
            risk: 0.9,
            type: 'repeated_restarts',
            data: { count: r.int(3, 6), window_ms: 10 * MINUTE },
          });
        }
        if (r.chance(0.12 * device.riskiness)) {
          events.push({ ts: mid + 2 * MINUTE, risk: r.range(0.42, 0.6), type: 'capture_failed' });
        }
        if (index === sessions.length - 1 && r.chance(0.35 * device.riskiness)) {
          // Stopped monitoring early, then turned it back on later.
          events.push({ ts: session.end, risk: 0.9, type: 'user_stop' });
          events.push({ ts: session.end + r.int(10, 50) * MINUTE, risk: 0, type: 'user_start' });
        } else if (index === sessions.length - 1 && daysAgo > 0) {
          events.push({
            ts: session.end + MINUTE,
            type: 'system_logout',
            data: { utc_ms: session.end + MINUTE },
          });
        }

        // Anything scheduled past "now" hasn't happened yet.
        const kept = events.filter((e) => e.ts < now).sort((a, b) => a.ts - b.ts);
        if (kept.length === 0) continue;
        batches.push({
          device,
          events: kept,
          startTime: kept[0].ts,
          endTime: kept[kept.length - 1].ts,
        });
      }
    }
  }
  return batches;
}
