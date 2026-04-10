export function sendToast(message: string, isError = false) {
  if (typeof window === "undefined") return;
  const event = new CustomEvent("global-alert", {
    detail: { message, isError },
  });
  window.dispatchEvent(event);
}
