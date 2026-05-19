import { DigestFrequency } from './email-domain';

const DAY_MS = 24 * 60 * 60 * 1000;
const DIGEST_RUN_TOLERANCE_MS = 60 * 60 * 1000;

function get6amUtcMs(timezone: string, referenceMs: number): number {
  const dateStr = new Intl.DateTimeFormat('en-CA', { timeZone: timezone }).format(
    new Date(referenceMs),
  );
  const noonUtcMs = new Date(`${dateStr}T12:00:00Z`).getTime();
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: timezone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).formatToParts(new Date(noonUtcMs));

  const get = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? '0');
  const localMs = Date.UTC(
    get('year'),
    get('month') - 1,
    get('day'),
    get('hour'),
    get('minute'),
    get('second'),
  );
  const offsetMs = localMs - noonUtcMs;
  return new Date(`${dateStr}T06:00:00Z`).getTime() - offsetMs;
}

export function getDigestWindowForRun(input: {
  cadence: DigestFrequency;
  now: number;
  timezone: string;
}) {
  let end = get6amUtcMs(input.timezone, input.now);

  if (input.now < end) {
    end = get6amUtcMs(input.timezone, input.now - DAY_MS);
  }

  if (input.now < end || input.now >= end + DIGEST_RUN_TOLERANCE_MS) {
    return null;
  }

  if (input.cadence === 'weekly') {
    const weekday = new Intl.DateTimeFormat('en-US', {
      weekday: 'short',
      timeZone: input.timezone,
    }).format(new Date(end));
    if (weekday !== 'Mon') {
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
