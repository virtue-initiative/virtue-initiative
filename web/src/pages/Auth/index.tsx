import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { useAuth } from "../../context/auth";
import {
  derivePasswordMaterial,
  encryptData,
  generateRandomKeyBytes,
  generateUserKeyPair,
} from "../../crypto";
import { api } from "../../api";
import "./style.css";
import { ThemeButton } from "../../components/ThemeButton";
import {
  Alert,
  Button,
  Card,
  Dialog,
  DialogActions,
  DialogHeader,
  Field,
  Input,
  SegmentedControl,
} from "@virtueinitiative/shared-web";

type AuthMode = "login" | "signup" | "forgot" | "reset";

function navigate(path: string, replace = false) {
  if (typeof window === "undefined") {
    return;
  }

  const method = replace ? "replaceState" : "pushState";
  window.history[method]({}, "", path);
  window.dispatchEvent(new PopStateEvent("popstate"));
}

export function Auth({
  mode,
}: {
  mode: "login" | "signup" | "forgot-password";
}) {
  const { login, signup, rememberWrappingKey } = useAuth();
  const inviteToken = useMemo(() => {
    if (typeof window === "undefined") return "";
    return (
      new URLSearchParams(window.location.search).get("partner_invite_token") ??
      ""
    );
  }, []);
  const resetToken = useMemo(() => {
    if (typeof window === "undefined") return "";
    return new URLSearchParams(window.location.search).get("token") ?? "";
  }, []);
  const authMode: AuthMode =
    mode === "forgot-password" ? (resetToken ? "reset" : "forgot") : mode;
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [resendLoading, setResendLoading] = useState(false);
  const [resendCooldown, setResendCooldown] = useState(0);
  const resendCooldownRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [resetTokenValid, setResetTokenValid] = useState(!resetToken);
  const [signupVerificationEmail, setSignupVerificationEmail] = useState("");
  const signupVerificationDialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    if (!resetToken || authMode !== "reset") return;
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
  }, [authMode, resetToken]);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setError(null);
    setStatus(null);
    setLoading(true);

    try {
      if (authMode === "login") {
        await login(email, password);
      } else if (authMode === "signup") {
        if (password !== confirm) {
          throw new Error("Passwords do not match");
        }
        const result = await signup(
          email,
          password,
          name || undefined,
          inviteToken || undefined,
        );
        setStatus(null);
        setSignupVerificationEmail(result.email);
        signupVerificationDialogRef.current?.showModal();
        setEmail(result.email);
        setPassword("");
        setConfirm("");
      } else if (authMode === "forgot") {
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
        await rememberWrappingKey(rotatedKeys.wrappingKey);
        await login(email, password);
        if (typeof window !== "undefined") {
          window.sessionStorage.setItem(
            "virtue_global_link_message",
            JSON.stringify({
              message: "Password reset successfully.",
              isError: false,
            }),
          );
        }
        setPassword("");
        setConfirm("");
        navigate("/", true);
      }
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Something went wrong");
    } finally {
      setLoading(false);
    }
  }

  const EMAIL_NOT_VERIFIED_MSG = "Please verify your email before logging in.";

  async function handleResendVerification() {
    setResendLoading(true);
    try {
      await api.resendVerificationEmail(email);
      setResendCooldown(60);
      resendCooldownRef.current = setInterval(() => {
        setResendCooldown((prev) => {
          if (prev <= 1) {
            clearInterval(resendCooldownRef.current!);
            resendCooldownRef.current = null;
            return 0;
          }
          return prev - 1;
        });
      }, 1000);
    } finally {
      setResendLoading(false);
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

  const loginUrl = inviteToken
    ? `/login?partner_invite_token=${encodeURIComponent(inviteToken)}`
    : "/login";
  const signupUrl = inviteToken
    ? `/signup?partner_invite_token=${encodeURIComponent(inviteToken)}`
    : "/signup";
  const forgotUrl = inviteToken
    ? `/forgot-password?partner_invite_token=${encodeURIComponent(inviteToken)}`
    : "/forgot-password";

  return (
    <div class="auth-page">
      <Card class="auth-card">
        <div class="auth-header">
          <h1 class="auth-title">The Virtue Initiative</h1>
          <ThemeButton />
        </div>
        <p class="auth-subtitle">Accountability starts here.</p>

        <Alert variant="error" class="auth-dev-warning">
          <strong>Warning:</strong> Virtue is currently in early development and
          does not work reliably.
        </Alert>

        {(authMode === "login" || authMode === "signup") && (
          <SegmentedControl
            segments={[
              { label: "Log in", value: "login" },
              { label: "Sign up", value: "signup" },
            ]}
            value={authMode}
            onChange={(value) =>
              navigate(value === "login" ? loginUrl : signupUrl)
            }
            class="auth-tabs"
          />
        )}

        {authMode === "forgot" && (
          <p class="hint-text auth-flow-hint">
            Enter your email to receive a password reset link.
          </p>
        )}
        {authMode === "reset" && (
          <>
            <p class="hint-text auth-flow-hint">
              Choose a new password to complete the reset for the account below.
            </p>
            <Alert variant="warning" class="auth-flow-hint">
              Resetting your password will generate a new encryption keypair for
              this account. Previously uploaded batches will remain
              inaccessible, and you should sign back in on your Virtue clients
              so future uploads use the new keys.
            </Alert>
          </>
        )}
        {inviteToken && (
          <p class="hint-text auth-flow-hint">
            This sign-in or sign-up will also accept your pending partner
            invite.
          </p>
        )}

        <form class="auth-form" onSubmit={handleSubmit}>
          {authMode === "signup" && (
            <Field label="Name (optional)">
              <Input
                type="text"
                value={name}
                onInput={(e) => setName((e.target as HTMLInputElement).value)}
                placeholder="Your name"
                autoComplete="name"
              />
            </Field>
          )}

          <Field label="Email">
            <Input
              type="email"
              value={email}
              onInput={(e) => setEmail((e.target as HTMLInputElement).value)}
              placeholder="you@example.com"
              autoComplete="email"
              required
              disabled={authMode === "reset"}
            />
          </Field>

          {authMode !== "forgot" && (
            <Field label={authMode === "reset" ? "New password" : "Password"}>
              <Input
                type="password"
                value={password}
                onInput={(e) =>
                  setPassword((e.target as HTMLInputElement).value)
                }
                placeholder={
                  authMode === "login"
                    ? "Enter your password"
                    : authMode === "reset"
                      ? "Choose a new password"
                      : "Choose a password"
                }
                autoComplete={
                  authMode === "login" ? "current-password" : "new-password"
                }
                required
                disabled={authMode === "reset" && !resetTokenValid}
              />
            </Field>
          )}

          {(authMode === "signup" || authMode === "reset") && (
            <Field label="Confirm password">
              <Input
                type="password"
                value={confirm}
                onInput={(e) =>
                  setConfirm((e.target as HTMLInputElement).value)
                }
                placeholder="Retype your password"
                autoComplete="new-password"
                required
                disabled={authMode === "reset" && !resetTokenValid}
              />
            </Field>
          )}

          {authMode === "signup" && (
            <p class="hint-text">
              During sign-up, Virtue creates an end-to-end encryption key for
              your account. It protects your uploaded logs, screenshots, and
              blocks so only you and partners you approve can decrypt them.
            </p>
          )}

          {status && <Alert variant="success">{status}</Alert>}
          {error && (
            <Alert variant="error">
              {error}
              {error === EMAIL_NOT_VERIFIED_MSG && (
                <>
                  {" "}
                  {resendLoading ? (
                    "Sending…"
                  ) : resendCooldown > 0 ? (
                    <>Try again in {resendCooldown}s.</>
                  ) : (
                    <button
                      type="button"
                      style="background:none;border:none;padding:0;font:inherit;color:inherit;text-decoration:underline;cursor:pointer;"
                      onClick={handleResendVerification}
                    >
                      Resend verification email
                    </button>
                  )}
                </>
              )}
            </Alert>
          )}

          <Button
            variant="primary"
            type="submit"
            class="auth-submit"
            disabled={loading}
          >
            {loading
              ? "Please wait…"
              : authMode === "login"
                ? "Log in"
                : authMode === "signup"
                  ? "Create account"
                  : authMode === "forgot"
                    ? "Send reset link"
                    : "Reset password"}
          </Button>
        </form>

        <div class="auth-links">
          {authMode === "login" && (
            <a class="auth-link" href={forgotUrl}>
              Forgot your password?
            </a>
          )}
          {(authMode === "forgot" || authMode === "reset") && (
            <a class="auth-link" href={loginUrl}>
              Back to log in
            </a>
          )}
        </div>

        <Dialog dialogRef={signupVerificationDialogRef}>
          <DialogHeader>Verify your email</DialogHeader>
          <p class="invite-desc">
            We sent a verification link to{" "}
            <strong>{signupVerificationEmail || "your email"}</strong>. You need
            to verify your email before you can log in.
          </p>
          <DialogActions>
            <Button
              variant="primary"
              type="button"
              onClick={() => signupVerificationDialogRef.current?.close()}
            >
              Got it
            </Button>
          </DialogActions>
        </Dialog>
      </Card>
    </div>
  );
}
