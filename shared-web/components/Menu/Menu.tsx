import { ComponentChildren } from 'preact';
import { useState, useRef, useEffect } from 'preact/hooks';
import { createPortal } from 'preact/compat';
import './Menu.css';

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
  placement?: 'bottom' | 'top';
};

export function Menu({ trigger, items, class: className, placement = 'bottom' }: MenuProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top?: number; bottom?: number; right: number } | null>(null);

  useEffect(() => {
    if (!open) return;

    function updatePosition() {
      const el = ref.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      if (placement === 'top') {
        setPos({
          bottom: window.innerHeight - rect.top + 4,
          right: window.innerWidth - rect.right,
        });
      } else {
        setPos({ top: rect.bottom, right: window.innerWidth - rect.right });
      }
    }
    updatePosition();

    function close(e: MouseEvent) {
      const target = e.target as Node;
      if (ref.current?.contains(target)) return;
      if (dropdownRef.current?.contains(target)) return;
      setOpen(false);
    }
    document.addEventListener('mousedown', close);
    window.addEventListener('resize', updatePosition);
    window.addEventListener('scroll', updatePosition, true);
    return () => {
      document.removeEventListener('mousedown', close);
      window.removeEventListener('resize', updatePosition);
      window.removeEventListener('scroll', updatePosition, true);
    };
  }, [open]);

  return (
    <div class={['vi-menu', className].filter(Boolean).join(' ')} ref={ref}>
      <div class="vi-menu__trigger" onClick={() => setOpen((o) => !o)}>
        {trigger}
      </div>
      {open &&
        pos &&
        createPortal(
          <div
            ref={dropdownRef}
            class={['vi-menu__dropdown', placement === 'top' && 'vi-menu__dropdown--top']
              .filter(Boolean)
              .join(' ')}
            role="menu"
            style={{ top: pos.top, bottom: pos.bottom, right: pos.right }}
          >
            {items.map((item, i) =>
              item.href ? (
                <a
                  key={i}
                  href={item.href}
                  class={['vi-menu__item', item.danger && 'vi-menu__item--danger']
                    .filter(Boolean)
                    .join(' ')}
                  role="menuitem"
                  onClick={() => setOpen(false)}
                >
                  {item.label}
                </a>
              ) : (
                <button
                  key={i}
                  type="button"
                  class={['vi-menu__item', item.danger && 'vi-menu__item--danger']
                    .filter(Boolean)
                    .join(' ')}
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
          </div>,
          document.body,
        )}
    </div>
  );
}
