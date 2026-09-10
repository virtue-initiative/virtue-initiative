import { useState } from 'preact/hooks';
import { JSX } from 'preact';
import { Alert, Field, Input } from '@virtueinitiative/shared-web';
import { EyeIcon, EyeSlashIcon } from './icons';
import './password-field.css';

type PasswordFieldProps = Omit<JSX.IntrinsicElements['input'], 'type' | 'id' | 'size'> & {
  label: string;
  id: string;
  error?: string;
  helpText?: string;
};

export function PasswordField({ label, id, error, helpText, ...inputProps }: PasswordFieldProps) {
  const [visible, setVisible] = useState(false);

  return (
    <Field label={label} id={id} error={error} helpText={helpText}>
      <div class="password-field">
        <Input id={id} type={visible ? 'text' : 'password'} error={!!error} {...inputProps} />
        <button
          type="button"
          class="password-field-toggle"
          onClick={() => setVisible((v) => !v)}
          aria-label={visible ? 'Hide password' : 'Show password'}
          aria-pressed={visible}
        >
          {visible ? <EyeSlashIcon /> : <EyeIcon />}
        </button>
      </div>
    </Field>
  );
}

export function PwnedPasswordWarning({
  count,
  class: className,
  linkClass,
}: {
  count: number | null;
  class?: string;
  linkClass?: string;
}) {
  if (!count) return null;
  return (
    <Alert variant="warning" class={className}>
      This password has appeared in {count.toLocaleString()} known data breaches. Choose a different
      one. Read more at{' '}
      <a
        class={linkClass}
        href="https://haveibeenpwned.com/Passwords"
        target="_blank"
        rel="noreferrer"
      >
        haveibeenpwned.com
      </a>
      .
    </Alert>
  );
}
