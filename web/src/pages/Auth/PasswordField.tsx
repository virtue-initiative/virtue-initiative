import { useState } from 'preact/hooks';
import { JSX } from 'preact';
import { Field, Input } from '@virtueinitiative/shared-web';
import { EyeIcon, EyeSlashIcon } from '../../components/icons';

type PasswordFieldProps = Omit<JSX.IntrinsicElements['input'], 'type' | 'id' | 'size'> & {
  label: string;
  id: string;
};

export function PasswordField({ label, id, ...inputProps }: PasswordFieldProps) {
  const [visible, setVisible] = useState(false);

  return (
    <Field label={label} id={id}>
      <div class="auth-password-field">
        <Input id={id} type={visible ? 'text' : 'password'} {...inputProps} />
        <button
          type="button"
          class="auth-password-toggle"
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
