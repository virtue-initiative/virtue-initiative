import { ComponentChildren, JSX } from "preact";
import "./Dialog.css";

type DialogProps = Omit<JSX.IntrinsicElements["dialog"], "ref"> & {
  children: ComponentChildren;
  dialogRef?: { current: HTMLDialogElement | null };
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
};

function mergeClasses(...classNames: Array<string | undefined>) {
  return classNames.filter(Boolean).join(" ");
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
      <path
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M6 18 18 6M6 6l12 12"
      />
    </svg>
  );
}

export function Dialog({
  children,
  dialogRef,
  class: className,
  onClick,
  ...props
}: DialogProps) {
  function handleClick(e: MouseEvent) {
    const dialog = e.currentTarget as HTMLDialogElement;
    if (e.target === e.currentTarget) {
      dialog.close();
    }
    (onClick as ((e: MouseEvent) => void) | undefined)?.(e);
  }

  return (
    <dialog
      {...props}
      ref={dialogRef}
      class={mergeClasses("vi-dialog", className as string | undefined)}
      onClick={handleClick}
    >
      {children}
    </dialog>
  );
}

export function DialogActions({
  children,
  left,
  class: className,
}: DialogActionsProps) {
  return (
    <div class={mergeClasses("vi-dialog-actions", className)}>
      {left && <div class="vi-dialog-actions-left">{left}</div>}
      <div class="vi-dialog-actions-right">{children}</div>
    </div>
  );
}

export function DialogSecondaryActions({
  children,
  class: className,
}: DialogSecondaryActionsProps) {
  return (
    <div class={mergeClasses("vi-dialog-secondary-actions", className)}>
      {children}
    </div>
  );
}

export function DialogHeader({ children, class: className }: DialogHeaderProps) {
  function closeDialog(e: MouseEvent) {
    (e.currentTarget as HTMLButtonElement).closest("dialog")?.close();
  }

  return (
    <div class={mergeClasses("vi-dialog-header", className)}>
      <h3 class="vi-dialog-title">{children}</h3>
      <button
        class="vi-dialog-close"
        type="button"
        aria-label="Close dialog"
        onClick={closeDialog}
      >
        <CloseIcon />
      </button>
    </div>
  );
}
