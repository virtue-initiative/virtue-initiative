import { useEffect, useMemo, useState } from "preact/hooks";
import { useAuth } from "../../context/auth";
import { useE2EE } from "../../context/e2ee";
import { api } from "../../api";
import {
  deriveWrappingKey,
  encryptData,
  encryptForPublicKey,
  exportPrivateKey,
  exportPublicKey,
  generateRandomKeyBytes,
  generateSharingKeyPair,
  hashPasswordForAuth,
} from "../../crypto";
import "./style.css";
import { ThemeButton } from "../../components/ThemeButton";

type AuthMode = "login" | "signup" | "forgot" | "reset";

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
  const [resetUserId, setResetUserId] = useState("");
  const [resetRequiresKeyRotation, setResetRequiresKeyRotation] =
    useState(false);
  const [resetPartnerTargets, setResetPartnerTargets] = useState<
    Array<{
      partnership_id: string;
      partner_email: string;
      partner_pub_key?: string;
    }>
  >([]);

  useEffect(() => {
    if (!resetToken) return;
    setLoading(true);
    api
      .validatePasswordResetToken(resetToken)
      .then((result) => {
        setEmail(result.email);
        setResetUserId(result.user_id);
        setResetRequiresKeyRotation(result.key_rotation_required);
        setResetPartnerTargets(result.partner_access_targets);
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
        const { access_token } = await login(email, password);
        if (inviteToken) {
          await api.acceptPartnerInvite(access_token, inviteToken);
          clearUrlToken("partner_invite_token");
        }
      } else if (mode === "signup") {
        if (password !== confirm) {
          throw new Error("Passwords do not match");
        }
        const {
          access_token,
          userId,
          wrappingKey: wk,
        } = await signup(email, password, name || undefined);
        await setupKeyMaterial(access_token, userId, wk);
        if (inviteToken) {
          await api.acceptPartnerInvite(access_token, inviteToken);
          clearUrlToken("partner_invite_token");
        }
        setStatus("Account created. Please verify your email address.");
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
        const passwordHash = await hashPasswordForAuth(password, email);
        const rotatedKeys = await buildResetKeyMaterial(password);
        await api.resetPassword(resetToken, passwordHash, rotatedKeys?.payload);
        if (rotatedKeys && resetUserId) {
          e2ee.clearKey();
          await rememberWrappingKey(rotatedKeys.wrappingKey);
        }
        setStatus(
          resetRequiresKeyRotation
            ? "Password updated. Older encrypted logs will stay unavailable, but once you sign in again your Virtue clients can resume uploading with fresh keys. Partners already monitoring this account will keep access to new logs automatically."
            : "Password updated. You can log in now.",
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

  async function setupKeyMaterial(token: string, uid: string, wk: CryptoKey) {
    const rawE2EE = generateRandomKeyBytes();
    const keyPair = await generateSharingKeyPair();
    const publicKey = await exportPublicKey(keyPair.publicKey);
    const privateKey = await exportPrivateKey(keyPair.privateKey);
    await e2ee.setKeyFromBytes(rawE2EE.buffer, uid);
    const encryptedE2EE = await encryptData(wk, rawE2EE);
    const encryptedPrivate = await encryptData(wk, privateKey);
    await api.updateUser(token, {
      e2ee_key: encryptedE2EE.toBase64(),
      pub_key: publicKey.toBase64(),
      priv_key: encryptedPrivate.toBase64(),
    });
  }

  async function buildResetKeyMaterial(newPassword: string) {
    if (!resetRequiresKeyRotation) {
      return undefined;
    }

    if (!resetUserId) {
      throw new Error("Reset token is missing account context");
    }

    const wrappingKey = await deriveWrappingKey(newPassword, resetUserId);
    const rawE2EE = generateRandomKeyBytes();
    const sharingKeyPair = await generateSharingKeyPair();
    const publicKey = await exportPublicKey(sharingKeyPair.publicKey);
    const privateKey = await exportPrivateKey(sharingKeyPair.privateKey);
    const partnerAccessKeys = await Promise.all(
      resetPartnerTargets
        .filter((target) => target.partner_pub_key)
        .map(async (target) => ({
          partnership_id: target.partnership_id,
          e2ee_key: (
            await encryptForPublicKey(
              Uint8Array.fromBase64(target.partner_pub_key!),
              rawE2EE,
            )
          ).toBase64(),
        })),
    );

    return {
      wrappingKey,
      payload: {
        e2ee_key: (await encryptData(wrappingKey, rawE2EE)).toBase64(),
        pub_key: publicKey.toBase64(),
        priv_key: (await encryptData(wrappingKey, privateKey)).toBase64(),
        partner_access_keys: partnerAccessKeys,
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
            {resetRequiresKeyRotation && (
              <p class="alert-error">
                Resetting your password will generate a new end-to-end
                encryption key for this account. Previously uploaded logs will
                remain inaccessible, and you should sign back in on your Virtue
                clients so future uploads use the new keys. Partners who already
                monitor this account will keep access to new logs automatically.
              </p>
            )}
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
                placeholder="Choose a password"
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
