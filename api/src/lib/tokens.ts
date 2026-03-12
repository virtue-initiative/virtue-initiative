import { createHash, randomBytes } from 'node:crypto';

export function generateOpaqueToken() {
  return randomBytes(24).toString('base64url');
}

export function hashOpaqueToken(token: string) {
  return createHash('sha256').update(token).digest('hex');
}
