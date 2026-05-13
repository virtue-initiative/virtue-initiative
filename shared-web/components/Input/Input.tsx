import { JSX } from "preact";
import "./Input.css";

type InputProps = JSX.IntrinsicElements["input"] & { error?: boolean };

export function Input({ error, class: className, ...props }: InputProps) {
  return (
    <input
      class={["vi-input", error && "vi-input--error", className]
        .filter(Boolean)
        .join(" ")}
      {...props}
    />
  );
}
