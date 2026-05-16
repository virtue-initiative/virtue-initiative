import { ComponentChildren } from 'preact';
import './Badge.css';

type BadgeVariant = 'green' | 'gray' | 'yellow' | 'red';

export function Badge({
  variant = 'gray',
  class: className,
  children,
}: {
  variant?: BadgeVariant;
  class?: string;
  children?: ComponentChildren;
}) {
  return (
    <span class={['vi-badge', `vi-badge--${variant}`, className].filter(Boolean).join(' ')}>
      {children}
    </span>
  );
}
