import { ComponentChildren, JSX } from 'preact';
import './Dialog.css';

type DialogProps = Omit<JSX.IntrinsicElements['dialog'], 'ref'> & {
  children: ComponentChildren;
  dialogRef?: { current: HTMLDialogElement | null };
  size?: 'md' | 'lg';
  /** While true, Escape and a click on the backdrop do not close the dialog (WEB-021). */
  busy?: boolean;
};

type DialogActionsProps = {
  children: ComponentChildren;
  left?: ComponentChildren;
  class?: string;
};

type DialogSecondaryActionsProps = {
  children: ComponentChildren;
  class?: string;
};

type DialogHeaderProps = {
  children: ComponentChildren;
  class?: string;
  actions?: ComponentChildren;
};

function mergeClasses(...classNames: Array<string | undefined>) {
  return classNames.filter(Boolean).join(' ');
}

function CloseIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      strokeWidth="1.5"
      stroke="currentColor"
      aria-hidden="true"
    >
      <path strokeLinecap="round" strokeLinejoin="round" d="M6 18 18 6M6 6l12 12" />
    </svg>
  );
}

export function Dialog({
  children,
  dialogRef,
  class: className,
  size,
  busy,
  onClick,
  onCancel,
  ...props
}: DialogProps) {
  function handleCancel(e: Event) {
    if (busy) e.preventDefault();
    (onCancel as ((e: Event) => void) | undefined)?.(e);
  }

  function handleClick(e: MouseEvent) {
    const dialog = e.currentTarget as HTMLDialogElement;
    if (e.target === e.currentTarget && !busy) {
      dialog.close();
    }
    (onClick as ((e: MouseEvent) => void) | undefined)?.(e);
  }

  return (
    <dialog
      {...props}
      ref={dialogRef}
      class={mergeClasses(
        'vi-dialog',
        size && `vi-dialog--${size}`,
        className as string | undefined,
      )}
      onClick={handleClick}
      onCancel={handleCancel}
    >
      {children}
    </dialog>
  );
}

export function DialogActions({ children, left, class: className }: DialogActionsProps) {
  return (
    <div class={mergeClasses('vi-dialog-actions', className)}>
      {left && <div class="vi-dialog-actions-left">{left}</div>}
      <div class="vi-dialog-actions-right">{children}</div>
    </div>
  );
}

export function DialogSecondaryActions({
  children,
  class: className,
}: DialogSecondaryActionsProps) {
  return <div class={mergeClasses('vi-dialog-secondary-actions', className)}>{children}</div>;
}

export function DialogHeader({ children, class: className, actions }: DialogHeaderProps) {
  function closeDialog(e: MouseEvent) {
    (e.currentTarget as HTMLButtonElement).closest('dialog')?.close();
  }

  return (
    <div class={mergeClasses('vi-dialog-header', className)}>
      <h3 class="vi-dialog-title">{children}</h3>
      {actions && <div class="vi-dialog-header-actions">{actions}</div>}
      <button class="vi-dialog-close" type="button" aria-label="Close dialog" onClick={closeDialog}>
        <CloseIcon />
      </button>
    </div>
  );
}
