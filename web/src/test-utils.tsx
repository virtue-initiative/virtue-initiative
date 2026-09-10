import { h } from 'preact';
import { render, RenderOptions } from '@testing-library/preact';
import { LocationProvider } from 'preact-iso';
import { ToastProvider } from '@virtueinitiative/shared-web';
import { vi } from 'vitest';
import { Session } from './utils/api/session';
import { APIProvider } from './utils/api';
import type { ComponentChildren } from 'preact';

export function makeFakeSession(overrides: Partial<Session> = {}): Session {
  return {
    userId: 'user-1',
    privateKey: null,
    wrappingKey: {} as CryptoKey,
    logout: vi.fn().mockResolvedValue(undefined),
    onTokenRefreshFailed: vi.fn(),
    isInvalidated: vi.fn().mockReturnValue(false),
    unwrapPrivateKey: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  } as unknown as Session;
}

export function mockSessionRestore(session: Session | null = makeFakeSession()) {
  return vi.spyOn(Session, 'restore').mockResolvedValue(session);
}

function AllProviders({ children }: { children: ComponentChildren }) {
  return (
    <LocationProvider>
      <ToastProvider>
        <APIProvider>{children}</APIProvider>
      </ToastProvider>
    </LocationProvider>
  );
}

export function renderWithClient(
  ui: h.JSX.Element,
  options?: Omit<RenderOptions, 'wrapper'>,
  session?: Session,
) {
  mockSessionRestore(session);
  return render(ui, { wrapper: AllProviders, ...options });
}
