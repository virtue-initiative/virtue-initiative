import { useState, useEffect } from 'preact/hooks';

export function LogImage({ token, imageUrl }: { token: string; imageUrl: string }) {
  const [imgOpen, setImgOpen] = useState(false);
  const [imgSrc, setImgSrc] = useState<string | null>(null);
  const [imgError, setImgError] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let objectUrl: string | null = null;

    setImgSrc(null);
    setImgError(false);

    fetch(imageUrl, {
      method: 'GET',
      credentials: 'include',
      headers: { Authorization: `Bearer ${token}` },
    })
      .then(async (res) => {
        if (!res.ok) throw new Error(`image fetch failed (${res.status})`);
        return res.blob();
      })
      .then((blob) => {
        if (cancelled) return;
        objectUrl = URL.createObjectURL(blob);
        setImgSrc(objectUrl);
      })
      .catch(() => { if (!cancelled) setImgError(true); });

    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [imageUrl, token]);

  if (imgError) return <div class="log-thumb-status">Unavailable</div>;
  if (!imgSrc) return <div class="log-thumb-status">Loading…</div>;

  return (
    <div class="log-thumb-wrap">
      <button class="log-thumb-btn" type="button" onClick={() => setImgOpen(true)} aria-label="View image">
        <img class="log-thumb" src={imgSrc} alt="capture" loading="lazy" />
      </button>
      {imgOpen && (
        <div class="img-overlay" onClick={() => setImgOpen(false)}>
          <img class="img-full" src={imgSrc} alt="capture" onClick={(e) => e.stopPropagation()} />
        </div>
      )}
    </div>
  );
}

export function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return 'Just now';
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}
