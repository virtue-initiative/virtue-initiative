import './Spinner.css';

type SpinnerSize = 'sm' | 'md' | 'lg';

export function Spinner({ size = 'md', class: className }: { size?: SpinnerSize; class?: string }) {
  return (
    <span
      class={['vi-spinner', size !== 'md' && `vi-spinner--${size}`, className]
        .filter(Boolean)
        .join(' ')}
      aria-label="Loading"
      role="status"
    />
  );
}
