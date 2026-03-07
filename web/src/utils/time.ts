import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";

dayjs.extend(relativeTime);

const dayHeadingFormatter = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  year: "numeric",
  month: "long",
  day: "numeric",
});

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  year: "numeric",
  month: "numeric",
  day: "numeric",
});

const timeFormatter = new Intl.DateTimeFormat(undefined, {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
});

export function formatRelativeTimestamp(timestamp: number | null): string {
  if (!timestamp) return "Never";
  return dayjs(timestamp).fromNow();
}

export function formatDate(timestamp: number): string {
  return dateFormatter.format(new Date(timestamp));
}

export function formatTime(timestamp: number): string {
  return timeFormatter.format(new Date(timestamp));
}

export function formatDayHeading(timestamp: number): string {
  const day = dayjs(timestamp).startOf("day");
  const today = dayjs().startOf("day");

  if (day.isSame(today)) return "Today";
  if (day.isSame(today.subtract(1, "day"))) return "Yesterday";

  return dayHeadingFormatter.format(new Date(timestamp));
}

export function localDateKey(timestamp: number): string {
  const date = new Date(timestamp);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
