import { useState } from 'preact/hooks';
import { useAuth } from '../../context/auth';
import { useE2EE } from '../../context/e2ee';
import { api } from '../../api';
import { deriveKey, encryptData, decryptBatch } from '../../crypto';
import './style.css';

function jwtSub(token: string): string | null {
  try {
    const b64 = token.split('.')[1].replace(/-/g, '+').replace(/_/g, '/');
    const padded = b64 + '='.repeat((4 - (b64.length % 4)) % 4);
    return JSON.parse(atob(padded)).sub ?? null;
  } catch {
    return null;
  }
}

export function Auth() {
  const { login, signup, wrappingKey } = useAuth();
  const e2ee = useE2EE();
  const [mode, setMode] = useState<'login' | 'signup'>('login');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [name, setName] = useState('');
  const [e2eePassword, setE2EEPassword] = useState('');
  const [confirmE2EE, setConfirmE2EE] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      if (mode === 'login') {
        const { access_token, userId } = await login(email, password);
        await restoreE2EEKeys(access_token, userId);
      } else {
        if (password !== confirm) {
          setError('Passwords do not match');
          setLoading(false);
          return;
        }
        if (e2eePassword !== confirmE2EE) {
          setError('E2EE passwords do not match');
          setLoading(false);
          return;
        }
        const { access_token, userId } = await signup(email, password, name || undefined);
        await setupE2EEKey(access_token, userId, e2eePassword);
      }
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Something went wrong');
    } finally {
      setLoading(false);
    }
  }

  async function setupE2EEKey(token: string, uid: string, e2eePass: string) {
    if (!wrappingKey) throw new Error('Wrapping key not available');
    // Derive the E2EE key and store in localStorage
    const e2eeKey = await deriveKey(e2eePass, uid, true);
    const rawE2EE = new Uint8Array(await crypto.subtle.exportKey('raw', e2eeKey));
    await e2ee.setKeyFromBytes(rawE2EE.buffer, uid);
    // Encrypt it with the wrapping key and upload to server
    const encrypted = await encryptData(wrappingKey, rawE2EE);
    await api.setE2EEKey(token, encrypted.toBase64());
  }

  async function restoreE2EEKeys(token: string, uid: string) {
    if (!wrappingKey) return;
    // Restore own E2EE key
    const { encryptedE2EEKey } = await api.getE2EEKey(token);
    if (encryptedE2EEKey) {
      const rawE2EE = await decryptBatch(wrappingKey, Uint8Array.fromBase64(encryptedE2EEKey));
      await e2ee.setKeyFromBytes(rawE2EE.buffer, uid);
    }
    // Restore partner E2EE keys
    const partners = await api.getPartners(token);
    await Promise.all(
      partners
        .filter((p) => p.status === 'accepted' && p.permissions.view_data && p.encryptedE2EEKey)
        .map(async (p) => {
          const rawKey = await decryptBatch(wrappingKey, Uint8Array.fromBase64(p.encryptedE2EEKey!));
          await e2ee.setKeyFromBytes(rawKey.buffer, p.partner_user_id);
        }),
    );
  }

  return (
    <div class="auth-page">
      <div class="auth-card">
        <h1 class="auth-title">The Virtue Initiative</h1>
        <p class="auth-subtitle">Accountability starts here.</p>

        <div class="auth-tabs">
          <button
            class={`auth-tab ${mode === 'login' ? 'active' : ''}`}
            onClick={() => { setMode('login'); setError(null); setConfirm(''); setE2EEPassword(''); setConfirmE2EE(''); }}
            type="button"
          >
            Log in
          </button>
          <button
            class={`auth-tab ${mode === 'signup' ? 'active' : ''}`}
            onClick={() => { setMode('signup'); setError(null); setConfirm(''); setE2EEPassword(''); setConfirmE2EE(''); }}
            type="button"
          >
            Sign up
          </button>
        </div>

        <form class="auth-form" onSubmit={handleSubmit}>
          {mode === 'signup' && (
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
                placeholder={mode === 'signup' ? 'Min. 8 characters' : '••••••••'}
                autoComplete={mode === 'login' ? 'current-password' : 'new-password'}
                required
              />
            </div>

          {mode === 'signup' && (
            <div class="field">
              <label for="confirm">Confirm password</label>
              <input
                id="confirm"
                type="password"
                value={confirm}
                onInput={(e) => setConfirm((e.target as HTMLInputElement).value)}
                placeholder="••••••••"
                autoComplete="new-password"
                required
              />
            </div>
          )}

          {mode === 'signup' && (
            <>
              <div class="field">
                <label for="e2ee-password">E2EE password</label>
                <input
                  id="e2ee-password"
                  type="password"
                  value={e2eePassword}
                  onInput={(e) => setE2EEPassword((e.target as HTMLInputElement).value)}
                  placeholder="Encryption password"
                  autoComplete="new-password"
                  required
                />
              </div>
              <div class="field">
                <label for="confirm-e2ee">Confirm E2EE password</label>
                <input
                  id="confirm-e2ee"
                  type="password"
                  value={confirmE2EE}
                  onInput={(e) => setConfirmE2EE((e.target as HTMLInputElement).value)}
                  placeholder="••••••••"
                  autoComplete="new-password"
                  required
                />
              </div>
            </>
          )}

          {error && <p class="auth-error">{error}</p>}

          <button class="btn btn-primary" type="submit" disabled={loading}>
            {loading ? 'Please wait…' : mode === 'login' ? 'Log in' : 'Create account'}
          </button>
        </form>
      </div>
    </div>
  );
}
