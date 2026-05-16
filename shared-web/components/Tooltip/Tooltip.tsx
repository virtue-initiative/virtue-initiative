import { ComponentChildren } from 'preact';
import './Tooltip.css';

type TooltipProps = {
  content: string;
  children: ComponentChildren;
  class?: string;
};

export function Tooltip({ content, children, class: className }: TooltipProps) {
  return (
    <span class={['vi-tooltip-wrapper', className].filter(Boolean).join(' ')}>
      {children}
      <span class="vi-tooltip" role="tooltip">
        {content}
      </span>
    </span>
  );
}
