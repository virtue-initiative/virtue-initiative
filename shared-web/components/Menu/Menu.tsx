import { ComponentChildren } from "preact";
import { useState, useRef, useEffect } from "preact/hooks";
import "./Menu.css";

type MenuItem = {
  label: string;
  onClick?: () => void;
  href?: string;
  danger?: boolean;
};
type MenuProps = {
  trigger: ComponentChildren;
  items: MenuItem[];
  class?: string;
};

export function Menu({ trigger, items, class: className }: MenuProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node))
        setOpen(false);
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <div class={["vi-menu", className].filter(Boolean).join(" ")} ref={ref}>
      <div class="vi-menu__trigger" onClick={() => setOpen((o) => !o)}>
        {trigger}
      </div>
      {open && (
        <div class="vi-menu__dropdown" role="menu">
          {items.map((item, i) =>
            item.href ? (
              <a
                key={i}
                href={item.href}
                class={["vi-menu__item", item.danger && "vi-menu__item--danger"]
                  .filter(Boolean)
                  .join(" ")}
                role="menuitem"
                onClick={() => setOpen(false)}
              >
                {item.label}
              </a>
            ) : (
              <button
                key={i}
                type="button"
                class={["vi-menu__item", item.danger && "vi-menu__item--danger"]
                  .filter(Boolean)
                  .join(" ")}
                role="menuitem"
                onClick={() => {
                  item.onClick?.();
                  setOpen(false);
                }}
              >
                {item.label}
              </button>
            ),
          )}
        </div>
      )}
    </div>
  );
}
