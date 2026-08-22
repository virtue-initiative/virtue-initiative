import { findUserByEmail } from './db';
import { decodeBase64 } from './encoding';
import { verifyPasswordAuth } from './password';

type User = NonNullable<Awaited<ReturnType<typeof findUserByEmail>>>;

export type VerifyCredentialsResult =
  | { status: 'ok'; user: User }
  | { status: 'invalid' }
  | { status: 'unverified'; user: User };

function decodePasswordAuth(value: string): ArrayBuffer | null {
  let decoded: ArrayBuffer;
  try {
    decoded = decodeBase64(value);
  } catch {
    return null;
  }
  return new Uint8Array(decoded).byteLength === 32 ? decoded : null;
}

export async function verifyUserCredentials(
  db: D1Database,
  email: string,
  passwordAuthBase64: string,
): Promise<VerifyCredentialsResult> {
  const normalizedEmail = email.trim().toLowerCase();
  const user = await findUserByEmail(db, normalizedEmail);

  const decodedPasswordAuth = decodePasswordAuth(passwordAuthBase64);
  if (!decodedPasswordAuth) {
    return { status: 'invalid' };
  }

  if (!user || !(await verifyPasswordAuth(decodedPasswordAuth, user.password_hash))) {
    return { status: 'invalid' };
  }

  if (user.email_verified !== 1) {
    return { status: 'unverified', user };
  }

  return { status: 'ok', user };
}
