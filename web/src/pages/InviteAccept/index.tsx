import { useEffect, useState } from 'preact/hooks';
import { api as rawApi, PartnerInviteValidation } from '../../utils/api/api';
import { describeError, useAPIContext } from '../../utils/api';
import { Alert, Button, Card, SegmentedControl } from '@virtueinitiative/shared-web';
import '../../styles/email-link.css';

function navigate(path: string) {
  if (typeof window === 'undefined') return;
  window.history.pushState({}, '', path);
  window.dispatchEvent(new PopStateEvent('popstate'));
}

const FREQUENCY_SEGMENTS = [
  { label: 'Daily', value: 'daily' },
  { label: 'Weekly', value: 'weekly' },
];

type InviteState =
  | { status: 'loading' }
  | { status: 'success'; ownerName?: string; ownerEmail: string; askFrequency: boolean }
  | { status: 'error'; message: string | null };

export function InviteAccept() {
  const apiClient = useAPIContext();
  const [state, setState] = useState<InviteState>({ status: 'loading' });
  const [frequency, setFrequency] = useState<'daily' | 'weekly'>('weekly');
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    if (typeof window === 'undefined' || !apiClient) return;
    const token = new URLSearchParams(window.location.search).get('partner_token');
    if (!token) {
      setState({ status: 'error', message: 'Missing invite token.' });
      return;
    }

    let owner: PartnerInviteValidation['owner'] | null = null;
    let hadAccepted = true;

    rawApi
      .validatePartnerInvite(token)
      .then((validation) => {
        owner = validation.owner;
      })
      .catch(() => {})
      .then(() => rawApi.getPartners())
      .then((partners) => {
        hadAccepted = partners.watching.some((p) => p.status === 'accepted');
      })
      .catch(() => {})
      .then(() => apiClient.acceptInvite(token))
      .then(() => {
        setState({
          status: 'success',
          ownerName: owner?.name,
          ownerEmail: owner?.email ?? '',
          askFrequency: !hadAccepted,
        });
      })
      .catch((err: unknown) => {
        setState({
          status: 'error',
          message: describeError(err, 'Failed to accept invite.'),
        });
      });
  }, [apiClient]);

  const saveFrequency = async () => {
    if (!apiClient) return;
    setSaving(true);
    setSaveError(null);
    try {
      await apiClient.updateSettings({ settings: { email_frequency: frequency } });
      navigate('/');
    } catch (err) {
      setSaveError(describeError(err, 'Failed to save your email preference.'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div class="email-link-page">
      <Card class="email-link-card">
        {state.status === 'loading' && <p>Processing invite…</p>}
        {state.status === 'success' && (
          <>
            <h2>Invite accepted</h2>
            <p>
              Invite from <strong>{state.ownerName ?? state.ownerEmail}</strong> accepted. You can
              now view their monitoring data.
            </p>
            {state.askFrequency ? (
              <>
                <p>
                  Summary emails list what happened on the devices you watch, so you can catch up
                  without opening the app. Choose how often you want them.
                </p>
                {saveError && <Alert variant="error">{saveError}</Alert>}
                <SegmentedControl
                  segments={FREQUENCY_SEGMENTS}
                  value={frequency}
                  onChange={(value) => setFrequency(value as 'daily' | 'weekly')}
                />
                <Button variant="primary" type="button" onClick={saveFrequency} disabled={saving}>
                  Done
                </Button>
                <p>
                  You can change this any time, or turn off emails, in your{' '}
                  <a
                    href="/settings"
                    onClick={(e) => {
                      e.preventDefault();
                      navigate('/settings');
                    }}
                  >
                    email settings
                  </a>
                  .
                </p>
              </>
            ) : (
              <Button variant="primary" type="button" onClick={() => navigate('/')}>
                Back to dashboard
              </Button>
            )}
          </>
        )}
        {state.status === 'error' && (
          <>
            <h2>Invite error</h2>
            {state.message && <p>{state.message}</p>}
            <Button variant="primary" type="button" onClick={() => navigate('/')}>
              Back to dashboard
            </Button>
          </>
        )}
      </Card>
    </div>
  );
}
