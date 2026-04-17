import { ComponentChildren, JSX, Ref } from "preact";

type DialogProps = Omit<JSX.HTMLAttributes<HTMLDialogElement>, "ref"> & {
  children: ComponentChildren;
  dialogRef?: Ref<HTMLDialogElement>;
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
  function handleClick(e: JSX.TargetedMouseEvent<HTMLDialogElement>) {
    if (e.target === e.currentTarget) {
      e.currentTarget.close();
    }

    onClick?.(e);
  }

  return (
    <dialog {...props} ref={dialogRef} class={className} onClick={handleClick}>
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
    <div class={mergeClasses("dialog-actions", className)}>
      {left && <div class="dialog-actions-left">{left}</div>}
      <div class="dialog-actions-right">{children}</div>
    </div>
  );
}

export function DialogSecondaryActions({
  children,
  class: className,
}: DialogSecondaryActionsProps) {
  return (
    <div class={mergeClasses("dialog-secondary-actions", className)}>
      {children}
    </div>
  );
}

export function DialogHeader({
  children,
  class: className,
}: DialogHeaderProps) {
  function closeDialog(e: JSX.TargetedMouseEvent<HTMLButtonElement>) {
    e.currentTarget.closest("dialog")?.close();
  }

  return (
    <div class={mergeClasses("dialog-header", className)}>
      <h3 class="dialog-title">{children}</h3>
      <button
        class="dialog-close"
        type="button"
        aria-label="Close dialog"
        onClick={closeDialog}
      >
        <CloseIcon />
      </button>
    </div>
  );
}
