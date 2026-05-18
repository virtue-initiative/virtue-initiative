export const DEFAULT_DIGEST_LOCAL_HOUR = 6;

const DAY_MINUTES = 24 * 60;

function normalizeHour(hour: number) {
  return ((Math.round(hour) % 24) + 24) % 24;
}

function normalizeMinutes(minutes: number) {
  return ((Math.round(minutes) % DAY_MINUTES) + DAY_MINUTES) % DAY_MINUTES;
}

export function localHourToUtcMinutes(hour: number, date = new Date()) {
  return normalizeMinutes(normalizeHour(hour) * 60 + date.getTimezoneOffset());
}

export function utcMinutesToLocalHour(utcMinutes: number, date = new Date()) {
  const localMinutes = normalizeMinutes(utcMinutes - date.getTimezoneOffset());
  return normalizeHour(Math.round(localMinutes / 60));
}

export function formatDigestHour(hour: number) {
  const normalized = normalizeHour(hour);
  const suffix = normalized >= 12 ? 'PM' : 'AM';
  const displayHour = normalized % 12 || 12;
  return `${displayHour}:00 ${suffix}`;
}
