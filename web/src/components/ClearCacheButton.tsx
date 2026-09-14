import { Button } from '@virtueinitiative/shared-web';
import { usePromise } from '../hooks/usePromise';
import { cacheClient } from '../utils/cache/client';

type ClearCacheButtonProps = {
  variant?: 'outline' | 'danger';
};

// Resets the local decrypted-log cache: every tab drops its cache worker, the OPFS files are
// deleted, and every tab reloads onto a fresh worker. Nothing on the server is lost, so there
// is no confirmation step; the app downloads and decrypts the logs again on the next view.
export function ClearCacheButton({ variant = 'outline' }: ClearCacheButtonProps) {
  const [clearing, setClearing] = usePromise();

  function clear() {
    if (!cacheClient) return;
    setClearing(
      cacheClient.resetCache().then(() => {
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
