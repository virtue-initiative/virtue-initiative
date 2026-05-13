import { ComponentChildren } from "preact";
import "./Card.css";

export function Card({
  highlight,
  class: className,
  children,
  ...props
}: {
  highlight?: boolean;
  class?: string;
  children?: ComponentChildren;
  [key: string]: any;
}) {
  const classes = ["vi-card", highlight && "vi-card--highlight", className]
    .filter(Boolean)
    .join(" ");
  return (
    <div class={classes} {...props}>
      {children}
    </div>
  );
}

export function CardHeader({
  class: className,
  children,
  ...props
}: {
  class?: string;
  children?: ComponentChildren;
  [key: string]: any;
}) {
  return (
    <div
      class={["vi-card-header", className].filter(Boolean).join(" ")}
      {...props}
    >
      {children}
    </div>
  );
}

export function CardActions({
  class: className,
  children,
  ...props
}: {
  class?: string;
  children?: ComponentChildren;
  [key: string]: any;
}) {
  return (
    <div
      class={["vi-card-actions", className].filter(Boolean).join(" ")}
      {...props}
    >
      {children}
    </div>
  );
}

export function CardGrid({
  class: className,
  children,
  ...props
}: {
  class?: string;
  children?: ComponentChildren;
  [key: string]: any;
}) {
  return (
    <div
      class={["vi-card-grid", className].filter(Boolean).join(" ")}
      {...props}
    >
      {children}
    </div>
  );
}
