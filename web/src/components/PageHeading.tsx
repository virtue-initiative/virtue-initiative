import type { ComponentChildren } from 'preact';
import './page-heading.css';

export function PageHeading({
  icon,
  children,
  actions,
}: {
  icon: ComponentChildren;
  children: ComponentChildren;
  actions?: ComponentChildren;
}) {
  return (
    <div class="page-heading">
      <span class="page-heading-icon">{icon}</span>
      <h1 class="page-heading-title">{children}</h1>
      {actions && <div class="page-heading-actions">{actions}</div>}
    </div>
  );
}
