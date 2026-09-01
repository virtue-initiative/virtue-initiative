import { Button } from '@virtueinitiative/shared-web';
import { usePromise } from '../hooks/usePromise';
import { cacheClient } from '../utils/cache/client';

type ClearCacheButtonProps = {
  variant?: 'outline' | 'danger';
};

// Wipes the local decrypted-log cache and reloads. Nothing on the server is lost, so there
// is no confirmation step: the app downloads and decrypts the logs again on the next view.
export function ClearCacheButton({ variant = 'outline' }: ClearCacheButtonProps) {
  const [clearing, setClearing] = usePromise();

  function clear() {
    if (!cacheClient) return;
    setClearing(
      cacheClient.clearCache().then(() => {
        window.location.reload();
      }),
    );
  }

  return (
    <Button variant={variant} type="button" onClick={clear} disabled={!cacheClient || clearing}>
      {clearing ? 'Clearing…' : 'Clear cache'}
    </Button>
  );
}
