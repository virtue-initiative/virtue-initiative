import { SignJWT, jwtVerify } from 'jose';

export type JWTType = 'access' | 'refresh' | 'server' | 'device-access' | 'device-refresh';

export interface JWTPayload {
  sub: string;
  type: JWTType;
  iat?: number;
  exp?: number;
}

function getSecretKey(secret: string) {
  return new TextEncoder().encode(secret);
}

export async function signJWT(
  payload: Omit<JWTPayload, 'iat' | 'exp'>,
  secret: string,
  expiresInSeconds: number,
): Promise<string> {
  return new SignJWT(payload)
    .setProtectedHeader({ alg: 'HS256' })
    .setIssuedAt()
    .setExpirationTime(Math.floor(Date.now() / 1000) + expiresInSeconds)
    .sign(getSecretKey(secret));
}

export async function verifyJWT(token: string, secret: string): Promise<JWTPayload> {
  const { payload } = await jwtVerify(token, getSecretKey(secret));

  if (typeof payload.sub !== 'string' || typeof payload.type !== 'string') {
    throw new Error('Invalid token payload');
  }

  return {
    sub: payload.sub,
    type: payload.type as JWTType,
    iat: payload.iat,
    exp: payload.exp,
  };
}

export function generateToken(
  type: JWTType,
  sub: string,
  secret: string,
  expiresInSeconds: number,
): Promise<string> {
  return signJWT({ sub, type }, secret, expiresInSeconds);
}

export function generateAccessToken(sub: string, secret: string, expiresInSeconds: number) {
  return generateToken('access', sub, secret, expiresInSeconds);
}

export function generateRefreshToken(sub: string, secret: string, expiresInSeconds: number) {
  return generateToken('refresh', sub, secret, expiresInSeconds);
}
