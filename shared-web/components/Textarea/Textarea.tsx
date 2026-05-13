import { JSX } from "preact";
import "./Textarea.css";

type TextareaProps = JSX.IntrinsicElements["textarea"] & {
  error?: boolean;
};

export function Textarea({ error, class: className, ...props }: TextareaProps) {
  return (
    <textarea
      class={["vi-textarea", error && "vi-textarea--error", className]
        .filter(Boolean)
        .join(" ")}
      {...props}
    />
  );
}
