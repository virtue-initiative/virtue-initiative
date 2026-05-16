import './Skeleton.css';

type SkeletonVariant = 'text' | 'rect' | 'circle';

export function Skeleton({
  variant = 'rect',
  width,
  height,
  class: className,
}: {
  variant?: SkeletonVariant;
  width?: string;
  height?: string;
  class?: string;
}) {
  return (
    <span
      class={['vi-skeleton', `vi-skeleton--${variant}`, className].filter(Boolean).join(' ')}
      style={{ width, height }}
      aria-hidden="true"
    />
  );
}
