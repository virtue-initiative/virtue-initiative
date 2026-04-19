import { DigestFrequency } from './email-domain';

export const DEFAULT_DIGEST_MINUTES_UTC = 6 * 60;
const DAY_MS = 24 * 60 * 60 * 1000;
const MINUTE_MS = 60 * 1000;
const DAY_MINUTES = 24 * 60;
const DIGEST_RUN_TOLERANCE_MS = 60 * MINUTE_MS;

function startOfUtcDay(timestamp: number) {
  const date = new Date(timestamp);
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate());
}

export function normalizeDigestMinutesUtc(minutes: number | null | undefined) {
  if (minutes == null || !Number.isInteger(minutes)) {
    return DEFAULT_DIGEST_MINUTES_UTC;
  }

  return ((minutes % DAY_MINUTES) + DAY_MINUTES) % DAY_MINUTES;
}

export function getDigestWindowForRun(input: {
  cadence: DigestFrequency;
  now: number;
  utcMinutes: number;
}) {
  const utcMinutes = normalizeDigestMinutesUtc(input.utcMinutes);
  let end = startOfUtcDay(input.now) + utcMinutes * MINUTE_MS;

  if (input.now < end) {
    end -= DAY_MS;
  }

  if (input.now < end || input.now >= end + DIGEST_RUN_TOLERANCE_MS) {
    return null;
  }

  if (input.cadence === 'weekly') {
    const dayOffset = (new Date(end).getUTCDay() + 6) % 7;
    end -= dayOffset * DAY_MS;

    if (input.now < end || input.now >= end + DIGEST_RUN_TOLERANCE_MS) {
      return null;
    }
  }

  return {
    start: end - (input.cadence === 'weekly' ? 7 * DAY_MS : DAY_MS),
    end,
  };
}

export function formatUtcDate(timestamp: number) {
  return new Date(timestamp).toISOString().slice(0, 10);
}
