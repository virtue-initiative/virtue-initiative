import { ComponentChildren, JSX } from 'preact';
import './IconButton.css';

type IconButtonProps = JSX.IntrinsicElements['button'] & {
  isActive?: boolean;
  children?: ComponentChildren;
};

export function IconButton({ isActive, class: className, children, ...props }: IconButtonProps) {
  const classes = ['vi-icon-btn', isActive && 'vi-icon-btn--active', className]
    .filter(Boolean)
    .join(' ');
  return (
    <button class={classes} type="button" {...props}>
      {children}
    </button>
  );
}
