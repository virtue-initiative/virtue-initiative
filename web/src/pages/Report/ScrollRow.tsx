import type { ComponentChildren } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import { ChevronLeftIcon, ChevronRightIcon } from '../Logs/log-icons';

/**
 * A titled row of items that scrolls sideways (swipe, trackpad or the arrow
 * buttons), snapping to each item.
 */
export function ScrollRow({
  label,
  count,
  children,
}: {
  label: string;
  count: number;
  children: ComponentChildren;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const [canPrev, setCanPrev] = useState(false);
  const [canNext, setCanNext] = useState(false);

  useEffect(() => {
    const track = trackRef.current;
    if (!track) return;
    const update = () => {
      setCanPrev(track.scrollLeft > 1);
      setCanNext(track.scrollLeft + track.clientWidth < track.scrollWidth - 1);
    };
    update();
    track.addEventListener('scroll', update, { passive: true });
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(update);
    observer?.observe(track);
    return () => {
      track.removeEventListener('scroll', update);
      observer?.disconnect();
    };
  }, [count]);

  const page = (direction: 1 | -1) => {
    const track = trackRef.current;
    if (!track) return;
    track.scrollBy({ left: direction * track.clientWidth * 0.85, behavior: 'smooth' });
  };

  return (
    <section class="report-row" aria-label={label}>
      <div class="report-row-header">
        <h3 class="report-row-title">
          {label} <span class="report-row-count">{count}</span>
        </h3>
        {(canPrev || canNext) && (
          <div class="report-row-nav">
            <button
              type="button"
              class="report-row-nav-button"
              aria-label={`Scroll ${label.toLowerCase()} back`}
              disabled={!canPrev}
              onClick={() => page(-1)}
            >
              <ChevronLeftIcon />
            </button>
            <button
              type="button"
              class="report-row-nav-button"
              aria-label={`Scroll ${label.toLowerCase()} forward`}
              disabled={!canNext}
              onClick={() => page(1)}
            >
              <ChevronRightIcon />
            </button>
          </div>
        )}
      </div>
      <div class="report-row-track" ref={trackRef}>
        {children}
      </div>
    </section>
  );
}
