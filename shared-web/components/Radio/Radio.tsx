import { JSX } from "preact";
import "./Radio.css";

type RadioProps = JSX.IntrinsicElements["input"] & { label?: string };

export function Radio({ label, class: className, id, ...props }: RadioProps) {
  const inputId = id ?? `radio-${Math.random().toString(36).slice(2)}`;
  return (
    <label class={["vi-radio", className].filter(Boolean).join(" ")}>
      <input type="radio" id={inputId} {...props} />
      {label && <span class="vi-radio__label">{label}</span>}
    </label>
  );
}
