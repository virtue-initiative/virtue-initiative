import { importPKCS8, importSPKI, jwtVerify, SignJWT } from 'jose';

export type JWTType = 'server' | 'device';

export interface JWTPayload {
  sub: string;
  type: JWTType;
  iat?: number;
  exp?: number;
}

const JWT_ALGORITHM = 'EdDSA';

type JWTPrivateKey = Awaited<ReturnType<typeof importPKCS8>>;
type JWTPublicKey = Awaited<ReturnType<typeof importSPKI>>;

const privateKeyCache = new Map<string, Promise<JWTPrivateKey>>();
const publicKeyCache = new Map<string, Promise<JWTPublicKey>>();

function normalizePem(pem: string) {
  const normalized = pem.replace(/\r\n/g, '\n').replace(/\\n/g, '\n').trim();

  if (!normalized) {
    throw new Error('JWT key must not be empty');
  }

  return normalized;
}

function getPrivateKey(privateKeyPem: string) {
  const normalized = normalizePem(privateKeyPem);
  let keyPromise = privateKeyCache.get(normalized);

  if (!keyPromise) {
    keyPromise = importPKCS8(normalized, JWT_ALGORITHM);
    privateKeyCache.set(normalized, keyPromise);
  }

  return keyPromise;
}

function getPublicKey(publicKeyPem: string) {
  const normalized = normalizePem(publicKeyPem);
  let keyPromise = publicKeyCache.get(normalized);

  if (!keyPromise) {
    keyPromise = importSPKI(normalized, JWT_ALGORITHM);
    publicKeyCache.set(normalized, keyPromise);
  }

  return keyPromise;
}

export async function signJWT(
  payload: Omit<JWTPayload, 'iat' | 'exp'>,
  privateKeyPem: string,
  expiresInSeconds: number,
): Promise<string> {
  return new SignJWT(payload)
    .setProtectedHeader({ alg: JWT_ALGORITHM })
    .setIssuedAt()
    .setExpirationTime(Math.floor(Date.now() / 1000) + expiresInSeconds)
    .sign(await getPrivateKey(privateKeyPem));
}

export async function verifyJWT(token: string, publicKeyPem: string): Promise<JWTPayload> {
  const { payload } = await jwtVerify(token, await getPublicKey(publicKeyPem), {
    algorithms: [JWT_ALGORITHM],
  });

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
  privateKeyPem: string,
  expiresInSeconds: number,
): Promise<string> {
  return signJWT({ sub, type }, privateKeyPem, expiresInSeconds);
}
