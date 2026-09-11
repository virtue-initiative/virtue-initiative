import type { ComponentChildren } from 'preact';
import './page-heading.css';

export function PageHeading({
  icon,
  children,
  after,
  actions,
}: {
  icon: ComponentChildren;
  children: ComponentChildren;
  /** Rendered beside the title — a status badge or similar qualifier. */
  after?: ComponentChildren;
  actions?: ComponentChildren;
}) {
  return (
    <div class="page-heading">
      <span class="page-heading-icon">{icon}</span>
      <h1 class="page-heading-title">{children}</h1>
      {after && <span class="page-heading-after">{after}</span>}
      {actions && <div class="page-heading-actions">{actions}</div>}
    </div>
  );
}
