import { ComponentChildren } from 'preact';
import './Field.css';

type FieldProps = {
  label: string;
  helpText?: string;
  error?: string;
  children?: ComponentChildren;
  class?: string;
};

export function Field({ label, helpText, error, children, class: className }: FieldProps) {
  return (
    <div class={['vi-field', error && 'vi-field--error', className].filter(Boolean).join(' ')}>
      <label class="vi-field__label">{label}</label>
      {children}
      {helpText && !error && <span class="vi-field__help">{helpText}</span>}
      {error && <span class="vi-field__error">{error}</span>}
    </div>
  );
}
