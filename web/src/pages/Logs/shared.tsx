import { useEffect, useState } from "preact/hooks";
import { DataLog } from "../../api";
import { BatchVerification } from "../../crypto";
import { formatDayHeading, localDateKey } from "../../utils/time";

export type FeedLog = DataLog & {
  batch_status: BatchVerification;
  source: "batch" | "log";
  image_w?: number;
  image_h?: number;
};

export interface LogDayGroup<T extends { ts: number }> {
  key: string;
  label: string;
  items: T[];
}

export function groupLogsByDay<T extends { ts: number }>(
  items: T[],
): LogDayGroup<T>[] {
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
  if (typeof value === "string") {
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
    typeof log.image_w === "number" &&
    typeof log.image_h === "number" &&
    log.image_w > 0 &&
    log.image_h > 0
  ) {
    return log.image_w / log.image_h;
  }
  return undefined;
}

export function getLogMetadata(log: DataLog) {
  return Object.entries(log.data)
    .filter(([key]) => key !== "image")
    .map(
      ([key, value]) =>
        [key, typeof value === "string" ? value : JSON.stringify(value)] as [
          string,
          string,
        ],
    );
}

export function describeRiskLevel(risk: number | undefined): string | null {
  if (typeof risk !== "number" || Number.isNaN(risk)) {
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
  return type.replace(/_/g, " ");
}

export function LogImage({
  imageBytes,
  onDimensions,
  previewTitle,
  previewSubtitle,
}: {
  imageBytes: Uint8Array;
  onDimensions?: (width: number, height: number) => void;
  previewTitle?: string;
  previewSubtitle?: string;
}) {
  const [imgSrc, setImgSrc] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [lightboxOpen, setLightboxOpen] = useState(false);

  useEffect(() => {
    const imageData = Uint8Array.from(imageBytes);
    const url = URL.createObjectURL(
      new Blob([imageData], { type: "image/webp" }),
    );
    setImgSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [imageBytes]);

  if (!imgSrc) return null;

  return (
    <>
      <button
        class="logs-thumb-button"
        type="button"
        onClick={() => setDialogOpen(true)}
        aria-label="View screenshot"
      >
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
      {dialogOpen && (
        <div class="logs-preview-overlay" onClick={() => setDialogOpen(false)}>
          <div
            class="logs-preview-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="Screenshot preview"
            onClick={(e) => e.stopPropagation()}
          >
            <div class="logs-preview-heading">
              <div class="logs-preview-header">
                {previewTitle && (
                  <p class="logs-preview-meta-title" aria-live="polite">
                    {previewTitle}
                  </p>
                )}
                <button
                  class="logs-preview-close"
                  type="button"
                  aria-label="Close screenshot preview"
                  onClick={() => setDialogOpen(false)}
                >
                  ×
                </button>
              </div>
              {previewSubtitle && (
                <p class="logs-preview-meta-subtitle">{previewSubtitle}</p>
              )}
            </div>
            <button
              class="logs-preview-image-button"
              type="button"
              onClick={() => {
                setDialogOpen(false);
                setLightboxOpen(true);
              }}
              aria-label="Open image in fullscreen"
            >
              <img class="logs-preview-image" src={imgSrc} alt="screenshot" />
            </button>
          </div>
        </div>
      )}
      {lightboxOpen && (
        <div
          class="logs-lightbox-overlay"
          onClick={() => setLightboxOpen(false)}
        >
          <div class="logs-lightbox-frame">
            <img
              class="logs-lightbox-image"
              src={imgSrc}
              alt="screenshot"
              onClick={(e) => e.stopPropagation()}
            />
          </div>
        </div>
      )}
    </>
  );
}
