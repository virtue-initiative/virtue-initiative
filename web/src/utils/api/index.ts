import { api } from './api';
import { APIClient } from './client';
import { Session } from './session';

export async function login(email: string, password: string): Promise<APIClient> {
  const session = await Session.fromLogin(email, password);
  return new APIClient(session);
}

export async function requestSignup(email: string, to?: string): Promise<void> {
  await api.signupRequest(email, to);
}

export async function finishSignup(
  verificationToken: string,
  name: string | undefined,
  password: string,
): Promise<APIClient> {
  const session = await Session.fromFinishSignup(verificationToken, name, password);
  return new APIClient(session);
}

export {
  APIProvider,
  useAPIContext,
  useSetAPIClient,
  useUser,
  usePartners,
  useDevices,
} from './hooks';

export { APIClient } from './client';
export type { LogQueryResult } from './client';
export type { User, Device, WatcherPartner, WatchingPartner } from './api';
export { api } from './api';
