import { ComponentChildren, JSX } from "preact";
import "./Select.css";

type SelectProps = Omit<JSX.IntrinsicElements["select"], "size"> & {
  error?: boolean;
  size?: "md" | "sm";
  children?: ComponentChildren;
};

export function Select({
  error,
  size,
  class: className,
  children,
  ...props
}: SelectProps) {
  return (
    <select
      class={[
        "vi-select",
        size && `vi-select--${size}`,
        error && "vi-select--error",
        className,
      ]
        .filter(Boolean)
        .join(" ")}
      {...props}
    >
      {children}
    </select>
  );
}
