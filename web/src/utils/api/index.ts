import { api } from './api';
import { APIClient } from './client';
import { Session } from './session';

export async function login(email: string, password: string): Promise<APIClient> {
  const session = await Session.fromLogin(email, password);
  return new APIClient(session);
}

export async function requestSignup(
  email: string,
  to?: string,
  turnstileToken?: string,
): Promise<void> {
  await api.signupRequest(email, to, turnstileToken);
}

function detectTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

export async function finishSignup(
  verificationToken: string,
  name: string | undefined,
  password: string,
  newsletterOptIn = false,
): Promise<APIClient> {
  const session = await Session.fromFinishSignup(
    verificationToken,
    name,
    password,
    newsletterOptIn,
    detectTimezone(),
  );
  return new APIClient(session);
}

export {
  APIProvider,
  useAPIContext,
  useSetAPIClient,
  useUser,
  usePartners,
  useDevices,
  usePasswords,
} from './hooks';

export { APIClient } from './client';
export type { LogQueryResult } from './client';
export type { User, Device, WatcherPartner, WatchingPartner, LockedPassword } from './api';
export { api, describeError, SERVER_ERROR_MESSAGE } from './api';
