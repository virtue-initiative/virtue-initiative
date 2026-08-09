import {
  calculateJwkThumbprint,
  exportJWK,
  importPKCS8,
  importSPKI,
  jwtVerify,
  SignJWT,
  type JWK,
} from 'jose';

export type JWTType = 'server' | 'hash-server' | 'device-cert';

export interface JWTPayload {
  sub: string;
  type: JWTType;
  pubkey?: string;
  iat?: number;
  exp?: number;
}

const JWT_ALGORITHM = 'EdDSA';
const JWT_CURVE = 'Ed25519';

type JWTPrivateKey = Awaited<ReturnType<typeof importPKCS8>>;
type JWTPublicKey = Awaited<ReturnType<typeof importSPKI>>;
type PublicJwk = JWK & {
  alg: typeof JWT_ALGORITHM;
  crv: typeof JWT_CURVE;
  kid: string;
  kty: 'OKP';
  use: 'sig';
  x: string;
};

const privateKeyCache = new Map<string, Promise<JWTPrivateKey>>();
const publicKeyCache = new Map<string, Promise<JWTPublicKey>>();
const publicJwkCache = new Map<string, Promise<PublicJwk>>();

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

export async function getPublicJwk(publicKeyPem: string) {
  const normalized = normalizePem(publicKeyPem);
  let jwkPromise = publicJwkCache.get(normalized);

  if (!jwkPromise) {
    jwkPromise = (async () => {
      const exported = await exportJWK(await getPublicKey(normalized));
      const kid = await calculateJwkThumbprint(exported);

      return {
        ...exported,
        alg: JWT_ALGORITHM,
        crv: JWT_CURVE,
        kid,
        kty: 'OKP',
        use: 'sig',
      } as PublicJwk;
    })();
    publicJwkCache.set(normalized, jwkPromise);
  }

  return jwkPromise;
}

export async function getJWKS(publicKeyPem: string) {
  return {
    keys: [await getPublicJwk(publicKeyPem)],
  };
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
    pubkey: typeof payload.pubkey === 'string' ? payload.pubkey : undefined,
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

export function generateDeviceCertToken(
  sub: string,
  pubkeyBase64: string,
  privateKeyPem: string,
  expiresInSeconds: number,
): Promise<string> {
  return signJWT(
    { sub, type: 'device-cert', pubkey: pubkeyBase64 },
    privateKeyPem,
    expiresInSeconds,
  );
}
