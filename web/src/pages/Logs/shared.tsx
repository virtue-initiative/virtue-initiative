import { useState, useEffect } from "preact/hooks";
import { BatchVerification } from "../../crypto";

export interface LogItem {
  id: string;
  taken_at: number; // ms epoch
  device_id: string;
  kind: string;
  image?: Uint8Array;
  metadata: [string, string][];
  batch_status: BatchVerification;
  source?: "batch" | "log";
}

export type ImageLogItem = LogItem & { image: Uint8Array };

export interface LogDayGroup<T extends { taken_at: number }> {
  key: string;
  label: string;
  items: T[];
}

const dayHeadingFormatter = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  year: "numeric",
  month: "long",
  day: "numeric",
});

function localDateKey(ts: number): string {
  const date = new Date(ts);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function groupLogsByDay<T extends { taken_at: number }>(
  items: T[],
): LogDayGroup<T>[] {
  const groups: LogDayGroup<T>[] = [];
  const byKey = new Map<string, LogDayGroup<T>>();

  for (const item of items) {
    const key = localDateKey(item.taken_at);
    let group = byKey.get(key);
    if (!group) {
      group = {
        key,
        label: dayHeadingFormatter.format(new Date(item.taken_at)),
        items: [],
      };
      byKey.set(key, group);
      groups.push(group);
    }
    group.items.push(item);
  }

  return groups;
}

export function LogImage({ imageBytes }: { imageBytes: Uint8Array }) {
  const [imgSrc, setImgSrc] = useState<string | null>(null);
  const [open, setOpen] = useState(false);

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
        class="log-thumb-btn"
        type="button"
        onClick={() => setOpen(true)}
        aria-label="View screenshot"
      >
        <img class="log-thumb" src={imgSrc} alt="screenshot" loading="lazy" />
      </button>
      {open && (
        <div class="img-overlay" onClick={() => setOpen(false)}>
          <img
            class="img-full"
            src={imgSrc}
            alt="screenshot"
            onClick={(e) => e.stopPropagation()}
          />
        </div>
      )}
    </>
  );
}
