import { SignJWT, jwtVerify } from 'jose';

export interface JWTPayload {
  sub: string; // user ID
  type: 'access' | 'refresh';
  iat?: number;
  exp?: number;
}

/**
 * Sign a JWT token
 */
export async function signJWT(
  payload: Omit<JWTPayload, 'iat' | 'exp'>,
  secret: string,
  expiresInSeconds: number
): Promise<string> {
  const secretKey = new TextEncoder().encode(secret);

  return await new SignJWT({ ...payload })
    .setProtectedHeader({ alg: 'HS256' })
    .setIssuedAt()
    .setExpirationTime(Math.floor(Date.now() / 1000) + expiresInSeconds)
    .sign(secretKey);
}

/**
 * Verify and decode a JWT token
 */
export async function verifyJWT(token: string, secret: string): Promise<JWTPayload> {
  const secretKey = new TextEncoder().encode(secret);

  try {
    const { payload } = await jwtVerify(token, secretKey);
    return {
      sub: payload.sub as string,
      type: payload.type as 'access' | 'refresh',
      iat: payload.iat,
      exp: payload.exp,
    };
  } catch (error) {
    throw new Error('Invalid or expired token');
  }
}

/**
 * Generate access token
 */
export async function generateAccessToken(userId: string, secret: string, expiry: number): Promise<string> {
  return signJWT({ sub: userId, type: 'access' }, secret, expiry);
}

/**
 * Generate refresh token
 */
export async function generateRefreshToken(userId: string, secret: string, expiry: number): Promise<string> {
  return signJWT({ sub: userId, type: 'refresh' }, secret, expiry);
}
