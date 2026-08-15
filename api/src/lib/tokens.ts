import { createHash, randomBytes } from 'node:crypto';

export type TokenPurpose =
  | 'web_session'
  | 'device_session'
  | 'signup'
  | 'email_change'
  | 'email_verification'
  | 'password_reset'
  | 'partner_invite';

const TOKEN_PREFIXES: Record<TokenPurpose, string> = {
  web_session: 'wst_',
  device_session: 'dst_',
  signup: 'sut_',
  email_change: 'ect_',
  email_verification: 'evt_',
  password_reset: 'prt_',
  partner_invite: 'pit_',
};

export function generateOpaqueToken(purpose: TokenPurpose) {
  return `${TOKEN_PREFIXES[purpose]}${randomBytes(24).toString('base64url')}`;
}

export function hashOpaqueToken(token: string) {
  return createHash('sha256').update(token).digest('hex');
}

export function assertTokenPurpose(token: string, purpose: TokenPurpose) {
  if (!token.startsWith(TOKEN_PREFIXES[purpose])) {
    throw new Error(`Invalid token purpose: expected ${purpose}`);
  }
}
