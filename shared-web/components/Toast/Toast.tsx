import { ToastItem } from "./useToast";

export function Toast({
  toast,
  onDismiss,
}: {
  toast: ToastItem;
  onDismiss: (id: string) => void;
}) {
  return (
    <div
      class={[
        "vi-toast",
        `vi-toast--${toast.variant}`,
        toast.closing && "vi-toast--closing",
      ]
        .filter(Boolean)
        .join(" ")}
      role="status"
    >
      <span class="vi-toast__message">{toast.message}</span>
      {toast.dismissible && (
        <button
          class="vi-toast__close"
          type="button"
          onClick={() => onDismiss(toast.id)}
          aria-label="Dismiss"
        >
          ×
        </button>
      )}
    </div>
  );
}
