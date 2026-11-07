import { Alert } from '@virtueinitiative/shared-web';
import { useEffect, useRef, useState } from 'preact/hooks';

// Unset in tests, so the widget renders nothing there. Dev uses Cloudflare's always-passing test
// key from .env.development.
export const TURNSTILE_SITE_KEY: string = import.meta.env.VITE_TURNSTILE_SITE_KEY || '';

const SCRIPT_URL = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';

interface TurnstileApi {
  render(
    el: HTMLElement,
    options: {
      sitekey: string;
      callback: (token: string) => void;
      'expired-callback': () => void;
      'error-callback': () => void;
    },
  ): string;
  remove(widgetId: string): void;
}

let scriptPromise: Promise<TurnstileApi> | undefined;

function loadTurnstile(): Promise<TurnstileApi> {
  scriptPromise ??= new Promise((resolve, reject) => {
    const script = document.createElement('script');
    script.src = SCRIPT_URL;
    script.async = true;
    script.onload = () => resolve((window as unknown as { turnstile: TurnstileApi }).turnstile);
    script.onerror = () => {
      scriptPromise = undefined;
      reject(new Error('Could not load Cloudflare Turnstile'));
    };
    document.head.appendChild(script);
  });
  return scriptPromise;
}

/**
 * Cloudflare Turnstile widget. Calls `onToken` with a token once the visitor passes, and with ''
 * when it expires or fails. A token is single use, so remount (change `key`) after each submit.
 * If the script can't load or the widget errors, it says so, since the form waits on a token.
 */
export function Turnstile({ onToken }: { onToken: (token: string) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!TURNSTILE_SITE_KEY) return;
    let widgetId: string | undefined;
    let cancelled = false;
    loadTurnstile()
      .then((turnstile) => {
        if (cancelled || !ref.current) return;
        widgetId = turnstile.render(ref.current, {
          sitekey: TURNSTILE_SITE_KEY,
          callback: (token) => {
            setFailed(false);
            onToken(token);
          },
          'expired-callback': () => onToken(''),
          'error-callback': () => {
            setFailed(true);
            onToken('');
          },
        });
      })
      .catch((err: unknown) => {
        console.error(err);
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
      if (widgetId) loadTurnstile().then((turnstile) => turnstile.remove(widgetId!));
    };
  }, []);

  if (!TURNSTILE_SITE_KEY) return null;
  return (
    <>
      <div ref={ref} class="turnstile" />
      {failed && (
        <Alert variant="error">
          The verification check could not load. Reload the page and try again.
        </Alert>
      )}
    </>
  );
}
