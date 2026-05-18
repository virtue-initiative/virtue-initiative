import { useEffect, useState } from 'preact/hooks';
import { api } from '../../utils/api';
import { Card, Button } from '@virtueinitiative/shared-web';
import '../Auth/style.css';
import './style.css';

type State = 'loading' | 'success' | 'error';

export function VerifyEmail() {
  const [state, setState] = useState<State>('loading');

  useEffect(() => {
    const token = new URLSearchParams(window.location.search).get('token');
    if (!token) {
      setState('error');
      return;
    }
    api
      .verifyEmail(token)
      .then(() => setState('success'))
      .catch(() => setState('error'));
  }, []);

  return (
    <div class="auth-page">
      <Card class="verify-email-card">
        {state === 'loading' && <p>Verifying…</p>}
        {state === 'success' && (
          <>
            <h2>Email updated</h2>
            <p>Your email address has been successfully updated.</p>
            <Button variant="primary" onClick={() => (window.location.href = '/settings')}>
              Go to settings
            </Button>
          </>
        )}
        {state === 'error' && <p>This verification link is invalid or has expired.</p>}
      </Card>
    </div>
  );
}
