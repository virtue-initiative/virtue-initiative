import { useEffect, useRef, useState } from 'preact/hooks';
import { DataLog } from '../../utils/api/api';
import { formatDate, formatTime } from '../../utils/time';
import { Button, Dialog, DialogHeader } from '@virtueinitiative/shared-web';
import { describeRiskLevel, getRiskLevel } from '@virtueinitiative/shared-web/risk';
import { InformationCircleIcon, LogIcon } from './log-icons';

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

export function getLogCategory(log: DataLog): string {
  const kind = log.data?.kind as string | undefined;
  const reason = log.data?.reason as string | undefined;
  switch (log.type) {
    case 'screenshot':
      return 'Screenshot';
    case 'screenshot_skipped':
      return 'Screenshot Skipped';
    case 'lifecycle':
      if (kind === 'computer_booted') return 'Computer Started';
      if (kind === 'computer_suspended') return 'Sleep';
      if (kind === 'computer_resumed') return 'Wake';
      if (kind === 'login') return 'Signed In';
      if (kind === 'logout') return 'Signed Out';
      if (kind === 'process_started') return 'Monitoring Started';
      if (kind === 'process_stopped_shutdown') return 'Computer Shut Down';
      if (kind === 'process_stopped_user' || kind === 'process_stopped_other')
        return 'Monitoring Stopped';
      if (kind === 'screenshot_paused') return 'Screenshots Paused';
      if (kind === 'screenshot_resumed') return 'Screenshots Resumed';
      return 'Activity';
    case 'lifecycle_alert':
      if (reason === 'ping_gap_while_running') return 'Unexpected Gap';
      if (reason === 'process_killed_before_shutdown') return 'Process Stopped Unexpectedly';
      if (reason === 'force_killed_before_shutdown') return 'Process Force-Stopped';
      if (reason === 'user_stopped_process') return 'Monitoring Stopped by User';
      if (reason === 'unexpected_process_start') return 'Unexpected Restart';
      if (reason === 'missing_resume') return 'Missing Wake Event';
      return 'Alert';
    case 'alert':
      return 'Alert';
    case 'capture_failed':
      return 'Capture Failed';
    case 'dev':
      return 'Developer';
    default:
      return (log.type ?? '').replace(/_/g, ' ');
  }
}

/** Base URL of the help page documenting every log type. */
export const LOG_TYPES_HELP_URL = 'https://virtueinitiative.org/help/web/log-types';

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
  const d = log.data;
  switch (log.type) {
    case 'lifecycle': {
      const kind = d.kind as string | undefined;
      if (kind === 'process_started') return `Monitoring started on ${deviceName}`;
      if (kind === 'process_stopped_user') return `Monitoring stopped by user on ${deviceName}`;
      if (kind === 'process_stopped_shutdown') return `${deviceName} shut down`;
      if (kind === 'process_stopped_other') return `Monitoring stopped on ${deviceName}`;
      if (kind === 'computer_suspended') return `${deviceName} went to sleep`;
      if (kind === 'computer_resumed') return `${deviceName} woke up`;
      if (kind === 'computer_booted') return `${deviceName} booted`;
      if (kind === 'login') return `User logged in on ${deviceName}`;
      if (kind === 'logout') return `User logged out on ${deviceName}`;
      if (kind === 'screenshot_paused') return `Screenshots paused on ${deviceName}`;
      if (kind === 'screenshot_resumed') return `Screenshots resumed on ${deviceName}`;
      return `Lifecycle event on ${deviceName}`;
    }
    case 'lifecycle_alert': {
      const reason = d.reason as string | undefined;
      if (reason === 'user_stopped_process') return `Monitoring stopped by user on ${deviceName}`;
      if (reason === 'unexpected_process_start')
        return `Unexpected restart detected on ${deviceName}`;
      if (reason === 'ping_gap_while_running') return `Monitoring gap detected on ${deviceName}`;
      if (reason === 'process_killed_before_shutdown')
        return `Process killed before shutdown on ${deviceName}`;
      if (reason === 'missing_resume') return `Missing resume event on ${deviceName}`;
      if (reason === 'force_killed_before_shutdown')
        return `Process force-killed before shutdown on ${deviceName}`;
      return `Alert on ${deviceName}`;
    }
    case 'screenshot':
      return `Screenshot captured on ${deviceName}`;
    case 'screenshot_skipped': {
      const reason = d.reason as string | undefined;
      if (reason === 'static_screen') return `Duplicate screenshot skipped on ${deviceName}`;
      if (reason === 'locked_or_screensaver')
        return `Screen locked, screenshot skipped on ${deviceName}`;
      return `Screenshot skipped on ${deviceName}`;
    }
    case 'alert': {
      const message = d.message as string | undefined;
      return message ?? `Alert on ${deviceName}`;
    }
    case 'capture_failed':
      return `Capture failed repeatedly on ${deviceName}`;
    case 'dev': {
      const title = d.title as string | undefined;
      const details = d.details as string | undefined;
      return title ? (details ? `${title}: ${details}` : title) : `Developer log on ${deviceName}`;
    }
    default:
      return `Event on ${deviceName}`;
  }
}

export const LOG_TYPES = [
  'screenshot',
  'screenshot_skipped',
  'lifecycle',
  'lifecycle_alert',
  'alert',
  'capture_failed',
  'dev',
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
  const riskLabel = describeRiskLevel(item.risk) ?? 'Risk unavailable';
  const riskBadge =
    getRiskLevel(item.risk) === 'high' ? (
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
              href={getLogHelpUrl(item)}
              target="_blank"
              rel="noreferrer"
              aria-label="Learn more about this event"
              title="Learn more about this event"
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
