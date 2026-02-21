import { SignJWT, jwtVerify } from 'jose';

export interface JWTPayload {
  sub: string; // user ID
  type: 'access' | 'refresh';
  iat?: number;
  exp?: number;
}

/**
 * Parse expiry string like "15m", "7d", "1h" to seconds
 */
function parseExpiry(expiry: string): number {
  const units: Record<string, number> = {
    s: 1,
    m: 60,
    h: 3600,
    d: 86400,
  };
  
  const match = expiry.match(/^(\d+)([smhd])$/);
  if (!match) {
    throw new Error(`Invalid expiry format: ${expiry}`);
  }
  
  const [, value, unit] = match;
  return parseInt(value) * units[unit];
}

/**
 * Sign a JWT token
 */
export async function signJWT(
  payload: Omit<JWTPayload, 'iat' | 'exp'>,
  secret: string,
  expiresIn: string
): Promise<string> {
  const secretKey = new TextEncoder().encode(secret);
  const expirySeconds = parseExpiry(expiresIn);
  
  return await new SignJWT({ ...payload })
    .setProtectedHeader({ alg: 'HS256' })
    .setIssuedAt()
    .setExpirationTime(Math.floor(Date.now() / 1000) + expirySeconds)
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
export async function generateAccessToken(userId: string, secret: string, expiry: string): Promise<string> {
  return signJWT({ sub: userId, type: 'access' }, secret, expiry);
}

/**
 * Generate refresh token
 */
export async function generateRefreshToken(userId: string, secret: string, expiry: string): Promise<string> {
  return signJWT({ sub: userId, type: 'refresh' }, secret, expiry);
}
