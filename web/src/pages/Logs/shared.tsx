import { useState, useEffect } from 'preact/hooks';
import { BatchVerification } from '../../crypto';

export interface LogItem {
  id: string;
  taken_at: number; // ms epoch
  device_id: string;
  kind: string;
  image?: Uint8Array;
  metadata: [string, string][];
  batch_status: BatchVerification;
}

export type ImageLogItem = LogItem & { image: Uint8Array };

export function LogImage({ imageBytes }: { imageBytes: Uint8Array }) {
  const [imgSrc, setImgSrc] = useState<string | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const url = URL.createObjectURL(new Blob([imageBytes], { type: 'image/webp' }));
    setImgSrc(url);
    return () => URL.revokeObjectURL(url);
  }, [imageBytes]);

  if (!imgSrc) return null;

  return (
    <>
      <button class="log-thumb-btn" type="button" onClick={() => setOpen(true)} aria-label="View screenshot">
        <img class="log-thumb" src={imgSrc} alt="screenshot" loading="lazy" />
      </button>
      {open && (
        <div class="img-overlay" onClick={() => setOpen(false)}>
          <img class="img-full" src={imgSrc} alt="screenshot" onClick={(e) => e.stopPropagation()} />
        </div>
      )}
    </>
  );
}
