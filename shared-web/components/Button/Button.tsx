import { ComponentChildren, JSX } from "preact";
import "./Button.css";

type ButtonVariant = "primary" | "outline" | "ghost" | "danger" | "flat";
type ButtonSize = "sm" | "md";

type ButtonProps = Omit<JSX.IntrinsicElements["button"], "size"> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  children?: ComponentChildren;
};

export function Button({
  variant = "ghost",
  size,
  class: className,
  children,
  ...props
}: ButtonProps) {
  const classes = [
    "vi-btn",
    variant && `vi-btn--${variant}`,
    size === "sm" && "vi-btn--sm",
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <button class={classes} {...props}>
      {children}
    </button>
  );
}
