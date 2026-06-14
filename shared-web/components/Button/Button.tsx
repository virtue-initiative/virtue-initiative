import { ComponentChildren, JSX } from 'preact';
import './Button.css';

type ButtonVariant = 'primary' | 'outline' | 'ghost' | 'danger' | 'flat';
type ButtonSize = 'sm' | 'md' | 'lg';

type ButtonProps = Omit<JSX.IntrinsicElements['button'], 'size'> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  children?: ComponentChildren;
  href?: string;
  target?: string;
  rel?: string;
};

export function Button({
  variant = 'ghost',
  size,
  class: className,
  children,
  href,
  target,
  rel,
  ...props
}: ButtonProps) {
  const classes = [
    'vi-btn',
    variant && `vi-btn--${variant}`,
    size === 'sm' && 'vi-btn--sm',
    size === 'lg' && 'vi-btn--lg',
    className,
  ]
    .filter(Boolean)
    .join(' ');

  if (href !== undefined) {
    return (
      <a
        href={href}
        class={classes}
        target={target}
        rel={rel}
        {...(props as unknown as JSX.IntrinsicElements['a'])}
      >
        {children}
      </a>
    );
  }

  return (
    <button class={classes} {...props}>
      {children}
    </button>
  );
}
