import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { api, finishSignup, login, requestSignup, useSetAPIClient } from '../../utils/api';
import {
  derivePasswordMaterial,
  encryptData,
  generateRandomKeyBytes,
  generateUserKeyPair,
} from '../../utils/api/crypto';
import './style.css';
import { ThemeButton } from '../../components/ThemeButton';
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
} from '@virtueinitiative/shared-web';

type AuthMode = 'login' | 'signup' | 'forgot' | 'reset' | 'finish-signup';

function navigate(path: string, replace = false) {
  if (typeof window === 'undefined') {
    return;
  }

  const method = replace ? 'replaceState' : 'pushState';
  window.history[method]({}, '', path);
  window.dispatchEvent(new PopStateEvent('popstate'));
}

export function Auth({ mode }: { mode: 'login' | 'signup' | 'forgot-password' }) {
  const setClient = useSetAPIClient();
  const toParam = useMemo(() => {
    if (typeof window === 'undefined') return '';
    return new URLSearchParams(window.location.search).get('to') ?? '';
  }, []);
  const signupToken = useMemo(() => {
    if (typeof window === 'undefined') return '';
    if (mode !== 'signup') return '';
    return new URLSearchParams(window.location.search).get('signup_token') ?? '';
  }, [mode]);
  const resetToken = useMemo(() => {
    if (typeof window === 'undefined') return '';
    return new URLSearchParams(window.location.search).get('token') ?? '';
  }, []);
  const authMode: AuthMode =
    mode === 'forgot-password'
      ? resetToken
        ? 'reset'
        : 'forgot'
      : mode === 'signup' && signupToken
        ? 'finish-signup'
        : mode;
  const [email, setEmail] = useState('');
  const [name, setName] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [resetTokenValid, setResetTokenValid] = useState(!resetToken);
  const [signupVerificationEmail, setSignupVerificationEmail] = useState('');
  const signupVerificationDialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    if (!resetToken || authMode !== 'reset') return;
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
        setError(err instanceof Error ? err.message : 'Reset token is invalid');
      })
      .finally(() => setLoading(false));
  }, [authMode, resetToken]);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setError(null);
    setStatus(null);
    setLoading(true);

    try {
      if (authMode === 'login') {
        const client = await login(email, password);
        if (toParam) {
          navigate(decodeURIComponent(toParam), true);
        }
        setClient(client);
      } else if (authMode === 'signup') {
        await requestSignup(email, toParam || undefined);
        setStatus(null);
        setSignupVerificationEmail(email);
        signupVerificationDialogRef.current?.showModal();
        setPassword('');
        setConfirm('');
      } else if (authMode === 'finish-signup') {
        if (!signupToken) {
          throw new Error('Signup token is missing');
        }
        if (password !== confirm) {
          throw new Error('Passwords do not match');
        }
        const client = await finishSignup(signupToken, name.trim() || undefined, password);
        setClient(client);
        setName('');
        setPassword('');
        setConfirm('');
        navigate(toParam ? decodeURIComponent(toParam) : '/', true);
      } else if (authMode === 'forgot') {
        await api.requestPasswordReset(email);
        setStatus('If that email exists, a reset link has been sent.');
      } else {
        if (!resetToken) {
          throw new Error('Reset token is missing');
        }
        if (!resetTokenValid) {
          throw new Error('Reset token is invalid or expired');
        }
        if (password !== confirm) {
          throw new Error('Passwords do not match');
        }
        const rotatedKeys = await buildResetKeyMaterial(password);
        await api.resetPassword(resetToken, rotatedKeys.payload);
        const client = await login(email, password);
        setClient(client);
        if (typeof window !== 'undefined') {
          window.sessionStorage.setItem(
            'virtue_global_link_message',
            JSON.stringify({
              message: 'Password reset successfully.',
              isError: false,
            }),
          );
        }
        setPassword('');
        setConfirm('');
        navigate('/', true);
      }
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Something went wrong');
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
        priv_key: (await encryptData(wrappingKey, keyPair.privateKey)).toBase64(),
      },
    };
  }

  const loginUrl = toParam ? `/login?to=${encodeURIComponent(toParam)}` : '/login';
  const signupUrl = toParam ? `/signup?to=${encodeURIComponent(toParam)}` : '/signup';
  const forgotUrl = toParam
    ? `/forgot-password?to=${encodeURIComponent(toParam)}`
    : '/forgot-password';

  return (
    <div class="auth-page">
      <Card class="auth-card">
        <div class="auth-header">
          <h1 class="auth-title">The Virtue Initiative</h1>
          <ThemeButton />
        </div>
        <p class="auth-subtitle">Accountability starts here.</p>

        <Alert variant="error" class="auth-dev-warning">
          <strong>Warning:</strong> Virtue is currently in early development and does not work
          reliably.
        </Alert>

        {(authMode === 'login' || authMode === 'signup') && (
          <SegmentedControl
            segments={[
              { label: 'Log in', value: 'login' },
              { label: 'Sign up', value: 'signup' },
            ]}
            value={authMode}
            onChange={(value) => navigate(value === 'login' ? loginUrl : signupUrl)}
            class="auth-tabs"
          />
        )}

        {authMode === 'forgot' && (
          <p class="hint-text auth-flow-hint">Enter your email to receive a password reset link.</p>
        )}
        {authMode === 'reset' && (
          <>
            <p class="hint-text auth-flow-hint">
              Choose a new password to complete the reset for the account below.
            </p>
            <Alert variant="warning" class="auth-flow-hint">
              Resetting your password will generate a new encryption keypair for this account.
              Previously uploaded batches will remain inaccessible, and you should sign back in on
              your Virtue clients so future uploads use the new keys.
            </Alert>
          </>
        )}
        {authMode === 'finish-signup' && (
          <p class="hint-text auth-flow-hint">
            Welcome! Finish creating your account by choosing a password.
          </p>
        )}

        <form class="auth-form" onSubmit={handleSubmit}>
          {authMode !== 'finish-signup' && (
            <Field label="Email">
              <Input
                type="email"
                value={email}
                onInput={(e) => setEmail((e.target as HTMLInputElement).value)}
                placeholder="you@example.com"
                autoComplete="email"
                required
                disabled={authMode === 'reset'}
              />
            </Field>
          )}

          {authMode === 'finish-signup' && (
            <Field label="Name">
              <Input
                type="text"
                value={name}
                onInput={(e) => setName((e.target as HTMLInputElement).value)}
                placeholder="Your name (optional)"
                autoComplete="name"
              />
            </Field>
          )}

          {authMode !== 'forgot' && authMode !== 'signup' && (
            <Field
              label={
                authMode === 'reset'
                  ? 'New password'
                  : authMode === 'finish-signup'
                    ? 'Choose a password'
                    : 'Password'
              }
            >
              <Input
                type="password"
                value={password}
                onInput={(e) => setPassword((e.target as HTMLInputElement).value)}
                placeholder={
                  authMode === 'login'
                    ? 'Enter your password'
                    : authMode === 'reset'
                      ? 'Choose a new password'
                      : 'Choose a password'
                }
                autoComplete={authMode === 'login' ? 'current-password' : 'new-password'}
                required
                disabled={authMode === 'reset' && !resetTokenValid}
              />
            </Field>
          )}

          {(authMode === 'reset' || authMode === 'finish-signup') && (
            <Field label="Confirm password">
              <Input
                type="password"
                value={confirm}
                onInput={(e) => setConfirm((e.target as HTMLInputElement).value)}
                placeholder="Retype your password"
                autoComplete="new-password"
                required
                disabled={authMode === 'reset' && !resetTokenValid}
              />
            </Field>
          )}

          {(authMode === 'signup' || authMode === 'finish-signup') && (
            <p class="hint-text">
              During sign-up, Virtue creates an end-to-end encryption key for your account. It
              protects your uploaded logs, screenshots, and blocks so only you and partners you
              approve can decrypt them.
            </p>
          )}

          {status && <Alert variant="success">{status}</Alert>}
          {error && <Alert variant="error">{error}</Alert>}

          <Button variant="primary" type="submit" class="auth-submit" disabled={loading}>
            {loading
              ? 'Please wait…'
              : authMode === 'login'
                ? 'Log in'
                : authMode === 'signup'
                  ? 'Send verification email'
                  : authMode === 'finish-signup'
                    ? 'Create account'
                    : authMode === 'forgot'
                      ? 'Send reset link'
                      : 'Reset password'}
          </Button>
        </form>

        <div class="auth-links">
          {authMode === 'login' && (
            <a class="auth-link" href={forgotUrl}>
              Forgot your password?
            </a>
          )}
          {(authMode === 'forgot' || authMode === 'reset') && (
            <a class="auth-link" href={loginUrl}>
              Back to log in
            </a>
          )}
        </div>

        <Dialog dialogRef={signupVerificationDialogRef}>
          <DialogHeader>Check your email</DialogHeader>
          <p class="invite-desc">
            We sent a verification link to{' '}
            <strong>{signupVerificationEmail || 'your email'}</strong>. Open the link from your
            email to finish setting up your account.
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
