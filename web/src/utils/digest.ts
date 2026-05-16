export const DEFAULT_DIGEST_LOCAL_HOUR = 6;

const DAY_MINUTES = 24 * 60;

function normalizeHour(hour: number) {
  return ((Math.round(hour) % 24) + 24) % 24;
}

function normalizeMinutes(minutes: number) {
  return ((Math.round(minutes) % DAY_MINUTES) + DAY_MINUTES) % DAY_MINUTES;
}

export function getBrowserTimeZone() {
  if (typeof Intl === 'undefined') {
    return '';
  }

  return Intl.DateTimeFormat().resolvedOptions().timeZone?.trim() ?? '';
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

export function formatUtcDigestMinutes(utcMinutes: number) {
  const normalized = normalizeMinutes(utcMinutes);
  const hour = Math.floor(normalized / 60)
    .toString()
    .padStart(2, '0');
  const minute = (normalized % 60).toString().padStart(2, '0');
  return `${hour}:${minute} UTC`;
}
