import { JSX } from 'preact';
import './Checkbox.css';

type CheckboxProps = JSX.IntrinsicElements['input'] & { label?: string };

export function Checkbox({ label, class: className, id, ...props }: CheckboxProps) {
  const inputId = id ?? `checkbox-${Math.random().toString(36).slice(2)}`;
  return (
    <label class={['vi-checkbox', className].filter(Boolean).join(' ')}>
      <input type="checkbox" id={inputId} {...props} />
      {label && <span class="vi-checkbox__label">{label}</span>}
    </label>
  );
}
