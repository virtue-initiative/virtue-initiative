import { useEffect } from "preact/hooks";
import { useAuth } from "../../context/auth";

const VERIFY_INFLIGHT_KEY = "virtue_verify_email_inflight";
const VERIFY_INFLIGHT_TTL_MS = 60_000;

function navigate(path: string, replace = false) {
  if (typeof window === "undefined") {
    return;
  }
  const method = replace ? "replaceState" : "pushState";
  window.history[method]({}, "", path);
  window.dispatchEvent(new PopStateEvent("popstate"));
}

function hardNavigate(path: string) {
  if (typeof window === "undefined") {
    return;
  }
  window.location.assign(path);
}

function hasRecentInflightVerification() {
  if (typeof window === "undefined") {
    return false;
  }

  const startedRaw = window.sessionStorage.getItem(VERIFY_INFLIGHT_KEY);
  if (!startedRaw) {
    return false;
  }

  const startedAt = Number(startedRaw);
  if (
    !Number.isFinite(startedAt) ||
    Date.now() - startedAt > VERIFY_INFLIGHT_TTL_MS
  ) {
    window.sessionStorage.removeItem(VERIFY_INFLIGHT_KEY);
    return false;
  }

  return true;
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
    const invite = params.get("partner_invite_token");

    if (!token) {
      if (hasRecentInflightVerification()) {
        return;
      }
      navigate("/", true);
      return;
    }

    window.sessionStorage.setItem(VERIFY_INFLIGHT_KEY, String(Date.now()));

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
        window.sessionStorage.removeItem(VERIFY_INFLIGHT_KEY);
        window.sessionStorage.setItem(
          "virtue_global_link_message",
          JSON.stringify({
            message: isEmailChange
              ? "Email changed successfully."
              : "Email verified successfully.",
            isError: false,
          }),
        );
        if (isEmailChange) {
          hardNavigate("/settings");
          return;
        }
        if (invite) {
          navigate(
            `/?partner_invite_token=${encodeURIComponent(invite)}`,
            true,
          );
          return;
        }
        navigate("/", true);
      })
      .catch((err: unknown) => {
        window.sessionStorage.removeItem(VERIFY_INFLIGHT_KEY);
        window.sessionStorage.setItem(
          "virtue_global_link_message",
          JSON.stringify({
            message:
              err instanceof Error ? err.message : "Failed to verify email",
            isError: true,
          }),
        );
        navigate("/login", true);
      });
  }, [verifyEmail]);

  return <div class="splash">Verifying email…</div>;
}
