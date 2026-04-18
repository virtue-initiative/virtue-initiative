import { useEffect } from "preact/hooks";
import { useAuth } from "../../context/auth";

function hardNavigate(path: string) {
  if (typeof window === "undefined") {
    return;
  }
  window.location.assign(path);
}

export function VerifyEmail() {
  const { verifyEmail } = useAuth();

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const params = new URLSearchParams(window.location.search);
    const token = params.get("token");
    const next = params.get("next");

    if (!token) {
      hardNavigate("/");
      return;
    }

    // Remove token from the URL before verification to avoid accidental retries.
    params.delete("token");
    const cleanPath = params.toString()
      ? `/verify-email?${params.toString()}`
      : "/verify-email";
    window.history.replaceState({}, "", cleanPath);

    verifyEmail(token)
      .then((result) => {
        const isEmailChange =
          result.purpose === "email_change" ||
          next === "settings" ||
          next === "/settings";
        window.sessionStorage.setItem(
          "virtue_global_link_message",
          JSON.stringify({
            message: isEmailChange
              ? "Email changed successfully."
              : "Email verified successfully.",
            isError: false,
          }),
        );
        hardNavigate(isEmailChange ? "/settings" : "/");
      })
      .catch((err: unknown) => {
        window.sessionStorage.setItem(
          "virtue_global_link_message",
          JSON.stringify({
            message:
              err instanceof Error ? err.message : "Failed to verify email",
            isError: true,
          }),
        );
        hardNavigate("/login");
      });
  }, [verifyEmail]);

  return <div class="splash">Verifying email…</div>;
}
