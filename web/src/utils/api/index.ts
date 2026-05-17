import { api } from './api';
import { APIClient } from './client';
import { Session } from './session';

export async function login(email: string, password: string): Promise<APIClient> {
  const session = await Session.fromLogin(email, password);
  return new APIClient(session);
}

export async function requestSignup(email: string, partnerInviteToken?: string): Promise<void> {
  await api.signupRequest(email, partnerInviteToken);
}

export async function finishSignup(
  verificationToken: string,
  name: string | undefined,
  password: string,
  partnerInviteToken?: string,
): Promise<APIClient> {
  const session = await Session.fromFinishSignup(
    verificationToken,
    name,
    password,
    partnerInviteToken,
  );
  return new APIClient(session);
}

export async function getSession(): Promise<APIClient | null> {
  const session = await Session.restore();
  return session ? new APIClient(session) : null;
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
export type { LogQuery, LogQueryResult, UserSettings, UpdateSettingsResult } from './client';
export type {
  User,
  Device,
  WatcherPartner,
  WatchingPartner,
  PartnerRelationships,
  Batch,
  DataLog,
  DataPage,
  LoginMaterial,
  HashParams,
  PasswordResetValidation,
  PartnerInviteValidation,
} from './api';
export { api, isToastHandledError } from './api';
