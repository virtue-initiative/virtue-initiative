import type { ComponentChildren } from 'preact';
import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import { useDocumentTitle } from '../hooks/useDocumentTitle';
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
  // The heading may be built from JSX ("Sam's logs — Laptop"), so the tab title is
  // read from what it rendered rather than passed in separately (WEB-002).
  const titleRef = useRef<HTMLHeadingElement>(null);
  const [text, setText] = useState<string | null>(null);
  useLayoutEffect(() => {
    setText(titleRef.current?.textContent ?? null);
  });
  useDocumentTitle(text);

  return (
    <div class="page-heading">
      <span class="page-heading-icon">{icon}</span>
      <h1 class="page-heading-title" ref={titleRef}>
        {children}
      </h1>
      {after && <span class="page-heading-after">{after}</span>}
      {actions && <div class="page-heading-actions">{actions}</div>}
    </div>
  );
}
