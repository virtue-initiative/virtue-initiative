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
  ChevronLeftIcon,
  ChevronRightIcon,
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
    category: 'Daily Check-in',
    icon: ActivityIcon,
    message: () =>
      'Once a day, your device sends a small update to confirm that monitoring is still active.',
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
  onPrev,
  onNext,
}: {
  item: FeedLog;
  deviceName: (id: string) => string;
  onClose: () => void;
  viewerId: string;
  /** Step to the previous log in the surrounding view; omitted at the start. */
  onPrev?: () => void;
  /** Step to the next log in the surrounding view; omitted at the end. */
  onNext?: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const advancedRef = useRef<HTMLDialogElement>(null);
  const [imgSrc, setImgSrc] = useState<string | null>(null);
  /** Object URL currently held by `imgSrc`, revoked only once it's replaced. */
  const imgUrlRef = useRef<string | null>(null);
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
    function onKeyDown(e: KeyboardEvent) {
      const tag = (e.target as HTMLElement | null)?.tagName?.toLowerCase();
      if (tag === 'input' || tag === 'select' || tag === 'textarea') return;
      // The Advanced dialog sits on top; let it have Escape and the arrows.
      if (advancedRef.current?.open) return;
      if (e.key === 'ArrowLeft' && onPrev) {
        e.preventDefault();
        onPrev();
      } else if (e.key === 'ArrowRight' && onNext) {
        e.preventDefault();
        onNext();
      }
    }
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [onPrev, onNext]);

  /** The step buttons live in the header, so they hold one spot regardless of
   * the log's type — they're rendered even at the ends of the list, disabled,
   * rather than disappearing and shifting the row. */
  const stepButtons = (
    <div class="logs-detail-step">
      <button
        class="logs-detail-step-button"
        type="button"
        aria-label="Previous log"
        disabled={!onPrev}
        onClick={() => onPrev?.()}
      >
        <ChevronLeftIcon />
      </button>
      <button
        class="logs-detail-step-button"
        type="button"
        aria-label="Next log"
        disabled={!onNext}
        onClick={() => onNext?.()}
      >
        <ChevronRightIcon />
      </button>
    </div>
  );

  useEffect(
    () => () => {
      if (imgUrlRef.current) URL.revokeObjectURL(imgUrlRef.current);
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;

    // Stepping with the arrows swaps `item` in place. The previous picture stays
    // up until the new one has decoded — clearing it first flashes an empty
    // dialog, and collapses its height, on every step.
    const show = (bytes: Uint8Array) => {
      if (cancelled) return;
      const url = URL.createObjectURL(
        new Blob([bytes as Uint8Array<ArrayBuffer>], { type: 'image/webp' }),
      );
      if (imgUrlRef.current) URL.revokeObjectURL(imgUrlRef.current);
      imgUrlRef.current = url;
      setImgSrc(url);
    };

    const clear = () => {
      if (cancelled) return;
      if (imgUrlRef.current) URL.revokeObjectURL(imgUrlRef.current);
      imgUrlRef.current = null;
      setImgSrc(null);
    };

    // Prefer inline image bytes (freshly decrypted, not yet persisted to IDB),
    // fall back to async IDB load for events already stored without inline image.
    const inlineBytes = getLogImage(item);
    if (inlineBytes) {
      show(inlineBytes);
    } else if (item.image_w !== undefined) {
      loadEventImage(viewerId, item.id)
        .then((bytes) => (bytes ? show(bytes) : clear()))
        .catch(clear);
    } else {
      clear();
    }

    return () => {
      cancelled = true;
    };
  }, [item.id, viewerId]);

  return (
    <Dialog dialogRef={dialogRef} size="lg" class="logs-detail-dialog">
      <DialogHeader
        actions={
          <>
            {stepButtons}
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
        <span class="logs-detail-title">
          <span class="logs-detail-title-icon">
            <LogIcon log={item} />
          </span>
          {getLogCategory(item)}
        </span>
      </DialogHeader>

      <div class="logs-detail-body">
        <div class="logs-detail-summary">
          <p class="logs-detail-message">{getLogMessage(item, deviceName(item.device_id))}</p>
          <p class="logs-detail-time">
            {formatDate(item.ts)} at {formatTime(item.ts)}
          </p>
        </div>
        <div class="logs-detail-media">
          {imgSrc ? (
            <img class="logs-detail-image" src={imgSrc} alt="screenshot" />
          ) : (
            /* A log with no picture gets an enlarged echo of its gallery tile,
               so the media area holds the same object you clicked. */
            <div class="logs-detail-preview">
              <span class="logs-detail-preview-icon">
                <LogIcon log={item} />
              </span>
              <span class="logs-detail-preview-type">{getLogCategory(item)}</span>
            </div>
          )}
        </div>
      </div>

      <div class="logs-detail-footer">
        {metadata.length > 0 && (
          <Button variant="ghost" type="button" onClick={() => advancedRef.current?.showModal()}>
            Advanced
          </Button>
        )}
        <Button variant="ghost" href={getLogHelpUrl(item)} target="_blank" rel="noreferrer">
          Learn more
        </Button>
      </div>

      <Dialog dialogRef={advancedRef} size="md" class="logs-advanced-dialog">
        <DialogHeader>Advanced</DialogHeader>
        <dl class="logs-meta logs-detail-meta">
          {metadata.map(([key, value], i) => (
            <>
              <dt key={`k-${i}`}>{key}</dt>
              <dd key={`v-${i}`}>{value}</dd>
            </>
          ))}
        </dl>
      </Dialog>
    </Dialog>
  );
}
