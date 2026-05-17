import { useEffect, useState } from 'preact/hooks';
import { DataLog } from '../../utils/api/api';
import { BatchVerification } from '../../utils/api/crypto';
import { formatDate, formatDayHeading, formatTime, localDateKey } from '../../utils/time';

export type FeedLog = DataLog & {
  batch_status: BatchVerification;
  source: 'batch' | 'log';
  image_w?: number;
  image_h?: number;
};

export interface LogDayGroup<T extends { ts: number }> {
  key: string;
  label: string;
  items: T[];
}

export function groupLogsByDay<T extends { ts: number }>(items: T[]): LogDayGroup<T>[] {
  const groups: LogDayGroup<T>[] = [];
  const byKey = new Map<string, LogDayGroup<T>>();

  for (const item of items) {
    const key = localDateKey(item.ts);
    let group = byKey.get(key);
    if (!group) {
      group = {
        key,
        label: formatDayHeading(item.ts),
        items: [],
      };
      byKey.set(key, group);
      groups.push(group);
    }
    group.items.push(item);
  }

  return groups;
}

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

export function getLogImageRatio(log: FeedLog): number | undefined {
  if (
    typeof log.image_w === 'number' &&
    typeof log.image_h === 'number' &&
    log.image_w > 0 &&
    log.image_h > 0
  ) {
    return log.image_w / log.image_h;
  }
  return undefined;
}

export function getLogMetadata(log: DataLog) {
  return Object.entries(log.data)
    .filter(([key]) => key !== 'image')
    .map(
      ([key, value]) =>
        [key, typeof value === 'string' ? value : JSON.stringify(value)] as [string, string],
    );
}

export function describeRiskLevel(risk: number | undefined): string | null {
  if (typeof risk !== 'number' || Number.isNaN(risk)) {
    return null;
  }

  const percentage = Math.round(Math.max(0, Math.min(1, risk)) * 100);

  if (risk > 0.7) {
    return `High risk (${percentage}%)`;
  }
  if (risk > 0.4) {
    return `Moderate risk (${percentage}%)`;
  }

  return `Risk ${percentage}%`;
}

export function humanizeLogType(type: string): string {
  return type.replace(/_/g, ' ');
}

export const LOG_TYPES = [
  'screenshot',
  'system_event',
  'lifecycle_alert',
  'lifecycle_marker',
  'lifecycle_transition',
  'developer_log',
] as const;

export function LogImage({
  imageBytes,
  onDimensions,
  onClick,
}: {
  imageBytes: Uint8Array;
  onDimensions?: (width: number, height: number) => void;
  onClick?: () => void;
}) {
  const [imgSrc, setImgSrc] = useState<string | null>(null);

  useEffect(() => {
    const imageData = Uint8Array.from(imageBytes);
    const url = URL.createObjectURL(new Blob([imageData], { type: 'image/webp' }));
    setImgSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [imageBytes]);

  if (!imgSrc) return null;

  return (
    <button class="logs-thumb-button" type="button" onClick={onClick} aria-label="View screenshot">
      <img
        class="logs-thumb-image"
        src={imgSrc}
        alt="screenshot"
        loading="lazy"
        onLoad={(event) => {
          if (!onDimensions) {
            return;
          }

          const image = event.currentTarget;
          if (image.naturalWidth > 0 && image.naturalHeight > 0) {
            onDimensions(image.naturalWidth, image.naturalHeight);
          }
        }}
      />
    </button>
  );
}

export function LogDetailDialog({
  item,
  deviceName,
  onClose,
}: {
  item: FeedLog;
  deviceName: (id: string) => string;
  onClose: () => void;
}) {
  const [imgSrc, setImgSrc] = useState<string | null>(null);
  const [lightboxOpen, setLightboxOpen] = useState(false);
  const imageBytes = getLogImage(item);
  const metadata = getLogMetadata(item);
  const riskLabel = describeRiskLevel(item.risk) ?? 'Risk unavailable';

  useEffect(() => {
    if (!imageBytes) return;
    const url = URL.createObjectURL(
      new Blob([imageBytes as Uint8Array<ArrayBuffer>], { type: 'image/webp' }),
    );
    setImgSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [imageBytes]);

  return (
    <>
      <div class="logs-detail-overlay" onClick={onClose}>
        <div
          class="logs-detail-dialog"
          role="dialog"
          aria-modal="true"
          aria-label="Log details"
          onClick={(e) => e.stopPropagation()}
        >
          <div class="logs-detail-header">
            <div>
              <span class="logs-type">{humanizeLogType(item.type)}</span>
              <span class="logs-device logs-device--indented">{deviceName(item.device_id)}</span>
            </div>
            <button class="logs-detail-close" type="button" aria-label="Close" onClick={onClose}>
              ×
            </button>
          </div>
          <p class="logs-detail-time">
            {formatDate(item.ts)} {formatTime(item.ts)}
          </p>
          <div class="logs-detail-badges">
            {item.risk > 0.7 ? (
              <span class="logs-verify-badge logs-verify-badge--failed">⚠ {riskLabel}</span>
            ) : item.risk > 0.4 ? (
              <span class="logs-verify-badge logs-verify-badge--moderate">{riskLabel}</span>
            ) : (
              <span class="logs-detail-risk-neutral">{riskLabel}</span>
            )}
            {item.batch_status === 'failed' && (
              <span class="logs-verify-badge logs-verify-badge--failed">⚠ Unverified</span>
            )}
          </div>
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
            <dl class="logs-meta logs-detail-meta">
              {metadata.map(([key, value], i) => (
                <>
                  <dt key={`k-${i}`}>{key}</dt>
                  <dd key={`v-${i}`}>{value}</dd>
                </>
              ))}
            </dl>
          )}
        </div>
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
    </>
  );
}
