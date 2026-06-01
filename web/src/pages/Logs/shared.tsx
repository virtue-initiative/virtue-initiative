import { useEffect, useRef, useState } from 'preact/hooks';
import { DataLog } from '../../utils/api/api';
import { BatchVerification } from '../../utils/api/crypto';
import { formatDate, formatTime } from '../../utils/time';
import { Dialog, DialogHeader } from '@virtueinitiative/shared-web';
import { describeRiskLevel, getRiskLevel } from '@virtueinitiative/shared-web/risk';
import { loadEventImage } from '../../utils/api/data-cache';

export type FeedLog = DataLog & {
  batch_status: BatchVerification;
  source: 'batch' | 'log';
  image_w?: number;
  image_h?: number;
};

export function toUint8Array(value: unknown): Uint8Array | undefined {
  if (!value) return undefined;
  if (value instanceof Uint8Array) return value;
  if (Array.isArray(value)) return new Uint8Array(value as number[]);
  if (typeof value === 'string') {
    try {
      return Uint8Array.fromBase64(value);
    } catch {
      return undefined;
    }
  }
  return undefined;
}

export function getLogImage(log: DataLog): Uint8Array | undefined {
  return toUint8Array(log.data.image);
}

function getLogMetadata(log: DataLog) {
  return Object.entries(log.data)
    .filter(([key]) => key !== 'image')
    .map(
      ([key, value]) =>
        [key, typeof value === 'string' ? value : JSON.stringify(value)] as [string, string],
    );
}

export function getLogCategory(type: string): string {
  switch (type) {
    case 'screenshot':
      return 'Screenshot';
    case 'lifecycle':
      return 'Lifecycle';
    case 'lifecycle_alert':
      return 'Alert';
    case 'alert':
      return 'Alert';
    case 'capture_failed':
      return 'System';
    case 'dev':
      return 'Developer';
    default:
      return type.replace(/_/g, ' ');
  }
}

export function getLogMessage(log: DataLog, deviceName: string): string {
  const d = log.data;
  switch (log.type) {
    case 'lifecycle': {
      const kind = d.kind as string | undefined;
      const sessionState = d.session_state as string | undefined;
      if (kind === 'process_started') return `Monitoring started on ${deviceName}`;
      if (kind === 'process_stopped_user') return `Monitoring stopped by user on ${deviceName}`;
      if (kind === 'process_stopped_shutdown') return `${deviceName} shut down`;
      if (kind === 'process_stopped_other') return `Monitoring stopped on ${deviceName}`;
      if (kind === 'computer_suspended') return `${deviceName} went to sleep`;
      if (kind === 'computer_resumed') return `${deviceName} woke up`;
      if (kind === 'user_session_changed') {
        if (sessionState === 'logged_in') return `User logged in on ${deviceName}`;
        if (sessionState === 'logged_out') return `User logged out on ${deviceName}`;
        return `User session changed on ${deviceName}`;
      }
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
      return `Alert on ${deviceName}`;
    }
    case 'screenshot':
      return `Screenshot captured on ${deviceName}`;
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
          </>
        }
      >
        {getLogCategory(item.type)}
      </DialogHeader>
      <p class="logs-detail-subtitle">
        On {deviceName(item.device_id)} at {formatDate(item.ts)} {formatTime(item.ts)}
      </p>
      <p class="logs-detail-message">{getLogMessage(item, deviceName(item.device_id))}</p>
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
