import { v4 as uuidv4 } from 'uuid';
import { createEmailToken, invalidateEmailTokens } from './db';
import { generateOpaqueToken, hashOpaqueToken } from './tokens';

export async function issueEmailToken(
  db: D1Database,
  user: { id: string; email: string },
  purpose: 'email_change' | 'email_verification' | 'password_reset',
  ttlMs: number,
) {
  await invalidateEmailTokens(db, user.id, purpose);
  const token = generateOpaqueToken(purpose);
  const id = uuidv4();
  const now = Date.now();

  await createEmailToken(db, {
    id,
    user_id: user.id,
    email: user.email,
    purpose,
    token_hash: hashOpaqueToken(token),
    expires_at: now + ttlMs,
    created_at: now,
  });

  return { id, token };
}
