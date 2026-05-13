import { ComponentChildren } from "preact";
import "./Alert.css";

type AlertVariant = "error" | "success" | "warning" | "info";

type AlertProps = {
  variant: AlertVariant;
  children?: ComponentChildren;
  class?: string;
};

export function Alert({ variant, children, class: className }: AlertProps) {
  return (
    <div
      class={["vi-alert", `vi-alert--${variant}`, className]
        .filter(Boolean)
        .join(" ")}
      role="alert"
    >
      {children}
    </div>
  );
}
