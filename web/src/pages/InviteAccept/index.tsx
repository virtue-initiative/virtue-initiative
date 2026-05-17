import { useEffect, useState } from 'preact/hooks';
import { api as rawApi, PartnerInviteValidation } from '../../utils/api/api';
import { useAPIContext } from '../../utils/api';
import { Button, Card } from '@virtueinitiative/shared-web';
import './style.css';

function navigate(path: string) {
  if (typeof window === 'undefined') return;
  window.history.pushState({}, '', path);
  window.dispatchEvent(new PopStateEvent('popstate'));
}

type InviteState =
  | { status: 'loading' }
  | { status: 'success'; ownerName?: string; ownerEmail: string }
  | { status: 'error'; message: string };

export function InviteAccept() {
  const apiClient = useAPIContext();
  const [state, setState] = useState<InviteState>({ status: 'loading' });

  useEffect(() => {
    if (typeof window === 'undefined' || !apiClient) return;
    const token = new URLSearchParams(window.location.search).get('partner_token');
    if (!token) {
      setState({ status: 'error', message: 'Missing invite token.' });
      return;
    }

    let owner: PartnerInviteValidation['owner'] | null = null;

    rawApi
      .validatePartnerInvite(token)
      .then((validation) => {
        owner = validation.owner;
      })
      .catch(() => {})
      .then(() => apiClient.acceptInvite(token))
      .then(() => {
        setState({
          status: 'success',
          ownerName: owner?.name,
          ownerEmail: owner?.email ?? '',
        });
      })
      .catch((err: unknown) => {
        setState({
          status: 'error',
          message: err instanceof Error ? err.message : 'Failed to accept invite.',
        });
      });
  }, [apiClient]);

  return (
    <div class="invite-accept-page">
      <Card class="invite-accept-card">
        {state.status === 'loading' && <p class="hint-text">Processing invite…</p>}
        {state.status === 'success' && (
          <>
            <h2>Invite accepted</h2>
            <p class="invite-desc">
              Invite from <strong>{state.ownerName ?? state.ownerEmail}</strong> accepted. You can
              now view their monitoring data.
            </p>
            <Button variant="primary" type="button" onClick={() => navigate('/')}>
              Back to dashboard
            </Button>
          </>
        )}
        {state.status === 'error' && (
          <>
            <h2>Invite error</h2>
            <p class="invite-desc">{state.message}</p>
            <Button variant="primary" type="button" onClick={() => navigate('/')}>
              Back to dashboard
            </Button>
          </>
        )}
      </Card>
    </div>
  );
}
