import { useEffect, useMemo, useState } from "preact/hooks";
import { useAuth } from "../../context/auth";
import { useE2EE } from "../../context/e2ee";
import { api } from "../../api";
import {
  derivePasswordMaterial,
  encryptData,
  generateRandomKeyBytes,
  generateUserKeyPair,
} from "../../crypto";
import "./style.css";
import { ThemeButton } from "../../components/ThemeButton";

type AuthMode = "login" | "signup" | "forgot" | "reset";
const GLOBAL_MESSAGE_KEY = "virtue_global_link_message";

export function Auth() {
  const { login, signup, rememberWrappingKey } = useAuth();
  const e2ee = useE2EE();
  const inviteToken = useMemo(() => {
    if (typeof window === "undefined") return "";
    return (
      new URLSearchParams(window.location.search).get("partner_invite_token") ??
      ""
    );
  }, []);
  const resetToken = useMemo(() => {
    if (typeof window === "undefined") return "";
    return (
      new URLSearchParams(window.location.search).get("reset_password_token") ??
      ""
    );
  }, []);
  const requestedSignupMode = useMemo(() => {
    if (typeof window === "undefined") return false;
    return window.location.hash === "#signup";
  }, []);
  const [mode, setMode] = useState<AuthMode>(
    resetToken ? "reset" : requestedSignupMode ? "signup" : "login",
  );
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [resetTokenValid, setResetTokenValid] = useState(!resetToken);

  useEffect(() => {
    if (!resetToken) return;
    setLoading(true);
    api
      .validatePasswordResetToken(resetToken)
      .then((result) => {
        setEmail(result.email);
        setResetTokenValid(true);
        setError(null);
      })
      .catch((err: unknown) => {
        setResetTokenValid(false);
        setError(err instanceof Error ? err.message : "Reset token is invalid");
      })
      .finally(() => setLoading(false));
  }, [resetToken]);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setError(null);
    setStatus(null);
    setLoading(true);

    try {
      if (mode === "login") {
        await login(email, password);
      } else if (mode === "signup") {
        if (password !== confirm) {
          throw new Error("Passwords do not match");
        }
        const { access_token, privateKey } = await signup(
          email,
          password,
          name || undefined,
        );
        e2ee.setPrivateKey(privateKey);
        if (typeof window !== "undefined") {
          window.sessionStorage.setItem(
            GLOBAL_MESSAGE_KEY,
            JSON.stringify({
              message:
                "Account created. Please check your email and spam folder for a verification email.",
              isError: false,
            }),
          );
        }
        setStatus(
          "Account created. Please check your email and spam folder for a verification email.",
        );
      } else if (mode === "forgot") {
        await api.requestPasswordReset(email);
        setStatus("If that email exists, a reset link has been sent.");
      } else {
        if (!resetToken) {
          throw new Error("Reset token is missing");
        }
        if (!resetTokenValid) {
          throw new Error("Reset token is invalid or expired");
        }
        if (password !== confirm) {
          throw new Error("Passwords do not match");
        }
        const rotatedKeys = await buildResetKeyMaterial(password);
        await api.resetPassword(resetToken, rotatedKeys.payload);
        e2ee.clearKey();
        await rememberWrappingKey(rotatedKeys.wrappingKey);
        setStatus(
          "Password updated. Older encrypted batches will stay unavailable after this key rotation, and new uploads will use the new keypair after you sign in again.",
        );
        setPassword("");
        setConfirm("");
        if (typeof window !== "undefined") {
          clearUrlToken("reset_password_token");
        }
        setMode("login");
      }
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Something went wrong");
    } finally {
      setLoading(false);
    }
  }

  async function buildResetKeyMaterial(newPassword: string) {
    const params = await api.getCurrentHashParams();
    const passwordSalt = generateRandomKeyBytes(params.salt_length);
    const { passwordAuth, wrappingKey } = await derivePasswordMaterial(
      newPassword,
      passwordSalt,
      params,
    );
    const keyPair = await generateUserKeyPair();

    return {
      wrappingKey,
      payload: {
        password_auth: passwordAuth.toBase64(),
        password_salt: passwordSalt.toBase64(),
        pub_key: keyPair.publicKey.toBase64(),
        priv_key: (
          await encryptData(wrappingKey, keyPair.privateKey)
        ).toBase64(),
      },
    };
  }

  function switchMode(nextMode: AuthMode) {
    setMode(nextMode);
    setError(null);
    setStatus(null);
    setConfirm("");
  }

  function clearUrlToken(key: string) {
    if (typeof window === "undefined") {
      return;
    }

    const nextUrl = new URL(window.location.href);
    nextUrl.searchParams.delete(key);
    window.history.replaceState({}, "", nextUrl.toString());
  }

  return (
    <div class="auth-page">
      <div class="card auth-card">
        <div class="auth-header">
          <h1 class="auth-title">The Virtue Initiative</h1>
          <ThemeButton />
        </div>
        <p class="auth-subtitle">Accountability starts here.</p>

        {(mode === "login" || mode === "signup") && (
          <div class="auth-tabs">
            <button
              class={`auth-tab ${mode === "login" ? "active" : ""}`}
              onClick={() => switchMode("login")}
              type="button"
            >
              Log in
            </button>
            <button
              class={`auth-tab ${mode === "signup" ? "active" : ""}`}
              onClick={() => switchMode("signup")}
              type="button"
            >
              Sign up
            </button>
          </div>
        )}

        {mode === "forgot" && (
          <p class="settings-hint">
            Enter your email to receive a password reset link.
          </p>
        )}
        {mode === "reset" && (
          <>
            <p class="settings-hint">
              Choose a new password to complete the reset for the account below.
            </p>
            <p class="alert-error">
              Resetting your password will generate a new encryption keypair for
              this account. Previously uploaded batches will remain
              inaccessible, and you should sign back in on your Virtue clients
              so future uploads use the new keys.
            </p>
          </>
        )}
        {inviteToken && (
          <p class="settings-hint">
            This sign-in or sign-up will also accept your pending partner
            invite.
          </p>
        )}

        <form class="auth-form" onSubmit={handleSubmit}>
          {mode === "signup" && (
            <>
              <div class="field">
                <label for="name">Name (optional)</label>
                <input
                  id="name"
                  type="text"
                  value={name}
                  onInput={(e) => setName((e.target as HTMLInputElement).value)}
                  placeholder="Your name"
                  autoComplete="name"
                />
              </div>
            </>
          )}

          <div class="field">
            <label for="email">Email</label>
            <input
              id="email"
              type="email"
              value={email}
              onInput={(e) => setEmail((e.target as HTMLInputElement).value)}
              placeholder="you@example.com"
              autoComplete="email"
              required
              disabled={mode === "reset"}
            />
          </div>

          {mode !== "forgot" && (
            <div class="field">
              <label for="password">
                {mode === "reset" ? "New password" : "Password"}
              </label>
              <input
                id="password"
                type="password"
                value={password}
                onInput={(e) =>
                  setPassword((e.target as HTMLInputElement).value)
                }
                placeholder={
                  mode === "login"
                    ? "Enter your password"
                    : mode === "reset"
                      ? "Choose a new password"
                      : "Choose a password"
                }
                autoComplete={
                  mode === "login" ? "current-password" : "new-password"
                }
                required
                disabled={mode === "reset" && !resetTokenValid}
              />
            </div>
          )}

          {(mode === "signup" || mode === "reset") && (
            <div class="field">
              <label for="confirm">Confirm password</label>
              <input
                id="confirm"
                type="password"
                value={confirm}
                onInput={(e) =>
                  setConfirm((e.target as HTMLInputElement).value)
                }
                placeholder="Retype your password"
                autoComplete="new-password"
                required
                disabled={mode === "reset" && !resetTokenValid}
              />
            </div>
          )}

          {mode === "signup" && (
            <p class="settings-hint">
              During sign-up, Virtue creates an end-to-end encryption key for
              your account. It protects your uploaded logs, screenshots, and
              blocks so only you and partners you approve can decrypt them.
            </p>
          )}

          {status && <p class="alert-success">{status}</p>}
          {error && <p class="alert-error">{error}</p>}

          <button
            class="btn btn-primary auth-submit"
            type="submit"
            disabled={loading}
          >
            {loading
              ? "Please wait…"
              : mode === "login"
                ? "Log in"
                : mode === "signup"
                  ? "Create account"
                  : mode === "forgot"
                    ? "Send reset link"
                    : "Reset password"}
          </button>
        </form>

        <div class="auth-links">
          {mode === "login" && (
            <button
              class="auth-link"
              type="button"
              onClick={() => switchMode("forgot")}
            >
              Forgot your password?
            </button>
          )}
          {mode === "forgot" && (
            <button
              class="auth-link"
              type="button"
              onClick={() => switchMode("login")}
            >
              Back to log in
            </button>
          )}
          {mode === "reset" && (
            <button
              class="auth-link"
              type="button"
              onClick={() => switchMode("login")}
            >
              Back to log in
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
