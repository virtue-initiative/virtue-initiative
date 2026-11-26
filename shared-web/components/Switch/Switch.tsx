import { JSX } from 'preact';
import './Switch.css';

type SwitchProps = Omit<JSX.IntrinsicElements['button'], 'onChange' | 'role' | 'type'> & {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
};

// A native button, so Space and Enter toggle it and it is focusable by default.
export function Switch({ label, checked, onChange, class: className, ...props }: SwitchProps) {
  return (
    <button
      {...props}
      type="button"
      role="switch"
      aria-checked={checked}
      class={['vi-switch', className].filter(Boolean).join(' ')}
      onClick={() => onChange(!checked)}
    >
      <span class="vi-switch__track" aria-hidden="true">
        <span class="vi-switch__thumb" />
      </span>
      <span class="vi-switch__label">{label}</span>
    </button>
  );
}
