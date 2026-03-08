import { useState } from "preact/hooks";
import { useAuth } from "../../context/auth";
import { useE2EE } from "../../context/e2ee";
import { api } from "../../api";
import {
  encryptData,
  exportPrivateKey,
  exportPublicKey,
  generateRandomKeyBytes,
  generateSharingKeyPair,
} from "../../crypto";
import "./style.css";
import { ThemeButton } from "../../components/ThemeButton";

export function Auth() {
  const { login, signup } = useAuth();
  const e2ee = useE2EE();
  const [mode, setMode] = useState<"login" | "signup">("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      if (mode === "login") {
        await login(email, password);
      } else {
        if (password !== confirm) {
          setError("Passwords do not match");
          setLoading(false);
          return;
        }
        const {
          access_token,
          userId,
          wrappingKey: wk,
        } = await signup(email, password, name || undefined);
        await setupKeyMaterial(access_token, userId, wk);
      }
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Something went wrong");
    } finally {
      setLoading(false);
    }
  }

  async function setupKeyMaterial(
    token: string,
    uid: string,
    wk: CryptoKey,
  ) {
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

  return (
    <div class="auth-page">
      <div class="card auth-card">
        <div class="auth-header">
          <h1 class="auth-title">The Virtue Initiative</h1>
          <ThemeButton />
        </div>
        <p class="auth-subtitle">Accountability starts here.</p>

        <div class="auth-tabs">
          <button
            class={`auth-tab ${mode === "login" ? "active" : ""}`}
            onClick={() => {
              setMode("login");
              setError(null);
              setConfirm("");
            }}
            type="button"
          >
            Log in
          </button>
          <button
            class={`auth-tab ${mode === "signup" ? "active" : ""}`}
            onClick={() => {
              setMode("signup");
              setError(null);
              setConfirm("");
            }}
            type="button"
          >
            Sign up
          </button>
        </div>

        <form class="auth-form" onSubmit={handleSubmit}>
          {mode === "signup" && (
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
            />
          </div>

          <div class="field">
            <label for="password">Password</label>
            <input
              id="password"
              type="password"
              value={password}
              onInput={(e) => setPassword((e.target as HTMLInputElement).value)}
              placeholder={mode === "signup" ? "Choose a password" : "••••••••"}
              autoComplete={
                mode === "login" ? "current-password" : "new-password"
              }
              required
            />
          </div>

          {mode === "signup" && (
            <div class="field">
              <label for="confirm">Confirm password</label>
              <input
                id="confirm"
                type="password"
                value={confirm}
                onInput={(e) =>
                  setConfirm((e.target as HTMLInputElement).value)
                }
                placeholder="••••••••"
                autoComplete="new-password"
                required
              />
            </div>
          )}

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
                : "Create account"}
          </button>
        </form>
      </div>
    </div>
  );
}
