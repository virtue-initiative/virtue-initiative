import { render, screen } from '@testing-library/preact';
import { afterEach, describe, expect, it, vi } from 'vitest';

afterEach(() => {
  vi.unstubAllEnvs();
  document.head.querySelectorAll('script[src*="turnstile"]').forEach((s) => s.remove());
});

describe('Turnstile (WEB-030)', () => {
  it('says so when the Turnstile script cannot load', async () => {
    // The site key is read when the module loads, and tests leave it unset.
    vi.stubEnv('VITE_TURNSTILE_SITE_KEY', 'test-site-key');
    vi.resetModules();
    const { Turnstile } = await import('./Turnstile');
    const onToken = vi.fn();
    render(<Turnstile onToken={onToken} />);

    const script = document.head.querySelector<HTMLScriptElement>('script[src*="turnstile"]');
    expect(script).not.toBeNull();
    script!.dispatchEvent(new Event('error'));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The verification check could not load. Reload the page and try again.',
    );
    expect(onToken).not.toHaveBeenCalled();
  });
});
