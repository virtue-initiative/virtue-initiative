import { ComponentChildren, JSX } from "preact";
import "./Select.css";

type SelectProps = JSX.IntrinsicElements["select"] & {
  error?: boolean;
  children?: ComponentChildren;
};

export function Select({
  error,
  class: className,
  children,
  ...props
}: SelectProps) {
  return (
    <select
      class={["vi-select", error && "vi-select--error", className]
        .filter(Boolean)
        .join(" ")}
      {...props}
    >
      {children}
    </select>
  );
}
