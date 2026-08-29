import type { JSX } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import { DataLog } from '../../utils/api/api';
import { formatDate, formatTime } from '../../utils/time';
import { Button, Dialog, DialogHeader } from '@virtueinitiative/shared-web';
import { describeRiskLevel, getRiskLevel } from '@virtueinitiative/shared-web/risk';
import { LANDING_URL } from '../../utils/landing-url';
import {
  ActivityIcon,
  BellAlertIcon,
  CameraIcon,
  ClockIcon,
  DocumentDuplicateIcon,
  ExclamationCircleIcon,
  ExclamationTriangleIcon,
  InformationCircleIcon,
  MoonIcon,
  SignInIcon,
  SignOutIcon,
  WrenchScrewdriverIcon,
} from './log-icons';

import { loadEventImage } from '../../utils/api/event-image';
import { type FeedLog, getLogImage, toUint8Array } from './types';
export type { FeedLog };
export { toUint8Array, getLogImage };

function getLogMetadata(log: DataLog) {
  return Object.entries(log.data)
    .filter(([key]) => key !== 'image')
    .map(
      ([key, value]) =>
        [key, typeof value === 'string' ? value : JSON.stringify(value)] as [string, string],
    );
}

type LogCaseKey =
  | 'screenshot'
  | 'screenshot_skipped'
  | 'screenshot_missed'
  | 'system_login'
  | 'system_logout'
  | 'suspend_detected'
  | 'lifecycle_other'
  | 'unexpected_gap'
  | 'unexpected_stop'
  | 'unexpected_start'
  | 'user_stop'
  | 'user_start'
  | 'repeated_restarts'
  | 'lifecycle_alert_other'
  | 'alert'
  | 'capture_failed'
  | 'dev'
  | 'heartbeat'
  | 'unknown';

/** The only place in the module that branches on a log's `type`/`kind`/`reason`. */
function getLogCaseKey(log: DataLog): LogCaseKey {
  const kind = log.data?.kind as string | undefined;
  const reason = log.data?.reason as string | undefined;
  switch (log.type) {
    case 'screenshot':
      return 'screenshot';
    case 'screenshot_skipped':
      return 'screenshot_skipped';
    case 'screenshot_missed':
      return 'screenshot_missed';
    case 'system_login':
      return 'system_login';
    case 'system_logout':
      return 'system_logout';
    case 'user_stop':
      return 'user_stop';
    case 'user_start':
      return 'user_start';
    case 'repeated_restarts':
      return 'repeated_restarts';
    // `lifecycle`/`lifecycle_alert` are the pre-rewrite client's wire shapes
    // — no longer sent, but kept here so already-stored logs still render.
    case 'lifecycle':
      if (kind === 'system_login') return 'system_login';
      if (kind === 'system_logout') return 'system_logout';
      if (kind === 'suspend_detected') return 'suspend_detected';
      return 'lifecycle_other';
    case 'lifecycle_alert':
      if (reason === 'unexpected_gap') return 'unexpected_gap';
      if (reason === 'unexpected_stop') return 'unexpected_stop';
      if (reason === 'unexpected_start') return 'unexpected_start';
      if (reason === 'user_stop') return 'user_stop';
      return 'lifecycle_alert_other';
    case 'alert':
      return 'alert';
    case 'capture_failed':
      return 'capture_failed';
    case 'dev':
      return 'dev';
    case 'heartbeat':
      return 'heartbeat';
    default:
      return 'unknown';
  }
}

const LOG_KIND_TABLE: Record<
  Exclude<LogCaseKey, 'unknown'>,
  {
    category: string;
    icon: () => JSX.Element;
    message: (deviceName: string, data: Record<string, unknown>) => string;
  }
> = {
  screenshot: {
    category: 'Screenshot',
    icon: CameraIcon,
    message: (d) => `Screenshot captured on ${d}`,
  },
  screenshot_skipped: {
    category: 'Screenshot Skipped',
    icon: DocumentDuplicateIcon,
    message: (d, data) => {
      const reason = data.reason as string | undefined;
      if (reason === 'static_screen') return `Duplicate screenshot skipped on ${d}`;
      if (reason === 'locked_or_screensaver') return `Screen locked, screenshot skipped on ${d}`;
      return `Screenshot skipped on ${d}`;
    },
  },
  screenshot_missed: {
    category: 'Screenshot Missed',
    icon: ClockIcon,
    message: (d) => `${d} missed a scheduled screenshot`,
  },
  system_login: {
    category: 'System Login',
    icon: SignInIcon,
    message: (d) => `${d} was logged into or started up`,
  },
  system_logout: {
    category: 'System Logout',
    icon: SignOutIcon,
    message: (d) => `${d} was logged out of or shut down`,
  },
  suspend_detected: {
    category: 'Suspend Detected',
    icon: MoonIcon,
    message: (d, data) => {
      const durationMs = typeof data.duration_ms === 'number' ? data.duration_ms : undefined;
      if (durationMs === undefined) return `${d} was asleep for a while`;
      const minutes = Math.round(durationMs / 60_000);
      const durationLabel = minutes >= 1 ? `${minutes} min` : `${Math.round(durationMs / 1000)}s`;
      return `${d} was asleep for ${durationLabel}`;
    },
  },
  lifecycle_other: {
    category: 'Activity',
    icon: ActivityIcon,
    message: (d) => `Lifecycle event on ${d}`,
  },
  unexpected_gap: {
    category: 'Unexpected Gap',
    icon: ClockIcon,
    message: (d) => `Monitoring gap detected on ${d}`,
  },
  unexpected_stop: {
    category: 'Process Stopped Unexpectedly',
    icon: ExclamationTriangleIcon,
    message: (d) => `Process stopped unexpectedly on ${d}`,
  },
  unexpected_start: {
    category: 'Unexpected Restart',
    icon: ExclamationTriangleIcon,
    message: (d) => `Unexpected restart detected on ${d}`,
  },
  user_stop: {
    category: 'Monitoring Stopped by User',
    icon: ExclamationTriangleIcon,
    message: (d) => `Monitoring stopped by user on ${d}`,
  },
  user_start: {
    category: 'Monitoring Resumed by User',
    icon: SignInIcon,
    message: (d) => `Monitoring resumed by user on ${d}`,
  },
  repeated_restarts: {
    category: 'Repeated Restarts',
    icon: ExclamationTriangleIcon,
    message: (d, data) => {
      const count = data.count as number | undefined;
      return count
        ? `${d} restarted ${count} times in a short window`
        : `${d} restarted repeatedly in a short window`;
    },
  },
  lifecycle_alert_other: {
    category: 'Alert',
    icon: ExclamationTriangleIcon,
    message: (d) => `Alert on ${d}`,
  },
  alert: {
    category: 'Alert',
    icon: BellAlertIcon,
    message: (d, data) => (data.message as string | undefined) ?? `Alert on ${d}`,
  },
  capture_failed: {
    category: 'Capture Failed',
    icon: ExclamationCircleIcon,
    message: (d) => `Capture failed repeatedly on ${d}`,
  },
  dev: {
    category: 'Developer',
    icon: WrenchScrewdriverIcon,
    message: (d, data) => {
      const title = data.title as string | undefined;
      const details = data.details as string | undefined;
      return title ? (details ? `${title}: ${details}` : title) : `Developer log on ${d}`;
    },
  },
  heartbeat: {
    category: 'Heartbeat',
    icon: ActivityIcon,
    message: (d) => `Heartbeat received from ${d}`,
  },
};

export const LOG_CATEGORIES = [...new Set(Object.values(LOG_KIND_TABLE).map((v) => v.category))];

export function getLogCategory(log: DataLog): string {
  const key = getLogCaseKey(log);
  if (key === 'unknown') return (log.type ?? '').replace(/_/g, ' ');
  return LOG_KIND_TABLE[key].category;
}

export function LogIcon({ log }: { log: DataLog }) {
  const key = getLogCaseKey(log);
  const IconComp = key === 'unknown' ? ActivityIcon : LOG_KIND_TABLE[key].icon;
  return <IconComp />;
}

/** Base URL of the help page documenting every log type. */
export const LOG_TYPES_HELP_URL = `${LANDING_URL}/help/web/log-types`;

/** URL of the help page explaining the concern percentage/levels. */
export const CONCERN_HELP_URL = `${LANDING_URL}/help/web/concern-scores`;

/** Slugified anchor for a log's section on the log-types help page. Mirrors the
 * id markdown generates from the matching heading (the category title). */
export function getLogHelpAnchor(log: DataLog): string {
  return getLogCategory(log)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

/** Deep link to the specific log type's section on the help page. */
export function getLogHelpUrl(log: DataLog): string {
  return `${LOG_TYPES_HELP_URL}#${getLogHelpAnchor(log)}`;
}

export function getLogMessage(log: DataLog, deviceName: string): string {
  const key = getLogCaseKey(log);
  if (key === 'unknown') return `Event on ${deviceName}`;
  return LOG_KIND_TABLE[key].message(deviceName, log.data);
}

export const LOG_TYPES = [
  'screenshot',
  'screenshot_skipped',
  'screenshot_missed',
  'system_login',
  'system_logout',
  'user_stop',
  'user_start',
  'repeated_restarts',
  'alert',
  'capture_failed',
  'dev',
  'heartbeat',
  // Pre-rewrite wire shapes, kept so already-stored logs still render.
  'lifecycle',
  'lifecycle_alert',
] as const;

const _dayLabelFmt = new Intl.DateTimeFormat(undefined, {
  weekday: 'long',
  month: 'short',
  day: 'numeric',
  year: 'numeric',
});
export function formatDayLabel(ms: number): string {
  return _dayLabelFmt.format(new Date(ms));
}

const _dayAndTimeFmt = new Intl.DateTimeFormat(undefined, {
  weekday: 'long',
  month: 'short',
  day: 'numeric',
  year: 'numeric',
  hour: 'numeric',
  minute: '2-digit',
});
export function formatDayAndTime(ms: number): string {
  return _dayAndTimeFmt.format(new Date(ms));
}

export function EventImage({
  eventId,
  viewerId,
  onClick,
}: {
  eventId: string;
  viewerId: string;
  onClick?: () => void;
}) {
  const [imgSrc, setImgSrc] = useState<string | null>(null);

  useEffect(() => {
    let url: string | null = null;
    loadEventImage(viewerId, eventId)
      .then((bytes) => {
        if (!bytes) return;
        url = URL.createObjectURL(
          new Blob([bytes as Uint8Array<ArrayBuffer>], { type: 'image/webp' }),
        );
        setImgSrc(url);
      })
      .catch(() => {});
    return () => {
      if (url) URL.revokeObjectURL(url);
    };
  }, [viewerId, eventId]);

  if (!imgSrc) return <div class="logs-thumb-placeholder" />;

  if (onClick) {
    return (
      <button
        class="logs-thumb-button"
        type="button"
        onClick={onClick}
        aria-label="View screenshot"
      >
        <img class="logs-thumb-image" src={imgSrc} alt="screenshot" loading="lazy" />
      </button>
    );
  }

  return <img class="logs-thumb-image" src={imgSrc} alt="" loading="lazy" />;
}

export function LogDetailDialog({
  item,
  deviceName,
  onClose,
  viewerId,
}: {
  item: FeedLog;
  deviceName: (id: string) => string;
  onClose: () => void;
  viewerId: string;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [imgSrc, setImgSrc] = useState<string | null>(null);
  const [lightboxOpen, setLightboxOpen] = useState(false);
  const metadata = getLogMetadata(item);
  const riskLabel = describeRiskLevel(item.risk) ?? 'Concern unavailable';
  const riskBadge =
    getRiskLevel(item.risk) === 'alert' ? (
      <span class="logs-verify-badge logs-verify-badge--failed">⚠ {riskLabel}</span>
    ) : getRiskLevel(item.risk) === 'high' ? (
      <span class="logs-verify-badge logs-verify-badge--failed">⚠ {riskLabel}</span>
    ) : getRiskLevel(item.risk) === 'medium' ? (
      <span class="logs-verify-badge logs-verify-badge--moderate">{riskLabel}</span>
    ) : (
      <span class="logs-detail-risk-neutral">{riskLabel}</span>
    );

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    dialog.showModal();
    dialog.addEventListener('close', onClose);
    return () => dialog.removeEventListener('close', onClose);
  }, []);

  useEffect(() => {
    // Prefer inline image bytes (freshly decrypted, not yet persisted to IDB),
    // fall back to async IDB load for events already stored without inline image.
    const inlineBytes = getLogImage(item);
    let url: string | null = null;
    let cancelled = false;

    const setUrl = (bytes: Uint8Array) => {
      if (cancelled) return;
      url = URL.createObjectURL(
        new Blob([bytes as Uint8Array<ArrayBuffer>], { type: 'image/webp' }),
      );
      setImgSrc(url);
    };

    if (inlineBytes) {
      setUrl(inlineBytes);
    } else if (item.image_w !== undefined) {
      loadEventImage(viewerId, item.id)
        .then((bytes) => {
          if (bytes) setUrl(bytes);
        })
        .catch(() => {});
    }

    return () => {
      cancelled = true;
      if (url) URL.revokeObjectURL(url);
    };
  }, [item.id, viewerId]);

  return (
    <Dialog dialogRef={dialogRef} size="lg" class="logs-detail-dialog">
      <DialogHeader
        actions={
          <>
            {riskBadge}
            {item.batch_status === 'failed' && (
              <span class="logs-verify-badge logs-verify-badge--failed">⚠ Unverified</span>
            )}
            <a
              class="logs-detail-help-link"
              href={CONCERN_HELP_URL}
              target="_blank"
              rel="noreferrer"
              aria-label="Learn more about the concern score"
              title="Learn more about the concern score"
            >
              <InformationCircleIcon />
            </a>
          </>
        }
      >
        {getLogCategory(item)}
      </DialogHeader>
      <p class="logs-detail-subtitle">
        On {deviceName(item.device_id)} at {formatDate(item.ts)} {formatTime(item.ts)}
      </p>
      <p class="logs-detail-message">{getLogMessage(item, deviceName(item.device_id))}</p>
      {!imgSrc && item.type !== 'screenshot' && (
        <div class="logs-detail-icon">
          <LogIcon log={item} />
        </div>
      )}
      {imgSrc && (
        <button
          class="logs-detail-image-button"
          type="button"
          onClick={() => setLightboxOpen(true)}
          aria-label="Open image fullscreen"
        >
          <img class="logs-detail-image" src={imgSrc} alt="screenshot" />
        </button>
      )}
      {metadata.length > 0 && (
        <details class="logs-detail-more">
          <summary>More details</summary>
          <dl class="logs-meta logs-detail-meta">
            {metadata.map(([key, value], i) => (
              <>
                <dt key={`k-${i}`}>{key}</dt>
                <dd key={`v-${i}`}>{value}</dd>
              </>
            ))}
          </dl>
        </details>
      )}
      <div class="logs-detail-learn-more">
        <Button variant="ghost" href={getLogHelpUrl(item)} target="_blank" rel="noreferrer">
          Learn more about this event
        </Button>
      </div>
      {lightboxOpen && (
        <div class="logs-lightbox-overlay" onClick={() => setLightboxOpen(false)}>
          <div class="logs-lightbox-frame">
            <img
              class="logs-lightbox-image"
              src={imgSrc!}
              alt="screenshot"
              onClick={(e) => e.stopPropagation()}
            />
          </div>
        </div>
      )}
    </Dialog>
  );
}
