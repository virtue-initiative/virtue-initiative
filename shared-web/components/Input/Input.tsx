import { JSX } from "preact";
import "./Input.css";

type InputProps = Omit<JSX.IntrinsicElements["input"], "size"> & {
  error?: boolean;
  size?: "md" | "sm";
};

export function Input({ error, size, class: className, ...props }: InputProps) {
  return (
    <input
      class={["vi-input", size && `vi-input--${size}`, error && "vi-input--error", className]
        .filter(Boolean)
        .join(" ")}
      {...props}
    />
  );
}
