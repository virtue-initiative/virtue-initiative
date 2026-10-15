import { z } from 'zod';

/** The iOS app's bundle ID. Staging and production builds share it. */
export const IOS_BUNDLE_ID = 'org.virtueinitiative.virtueios';

/** SHA-256 of Apple Root CA - G3 (API-057). */
export const APPLE_ROOT_CA_G3_SHA256 =
  '63343abfb89a6a03ebb57e9b3f5fa7be7c4f5c756f3017b3a8c488c3653e9179';

// Marker extensions Apple puts on the certificates that sign App Store data.
const LEAF_MARKER_OID = '1.2.840.113635.100.6.11.1';
const INTERMEDIATE_MARKER_OID = '1.2.840.113635.100.6.2.1';

const ECDSA_HASH_BY_OID: Record<string, 'SHA-256' | 'SHA-384'> = {
  '1.2.840.10045.4.3.2': 'SHA-256',
  '1.2.840.10045.4.3.3': 'SHA-384',
};
const CURVE_BY_OID: Record<string, { name: 'P-256' | 'P-384'; size: number }> = {
  '1.2.840.10045.3.1.7': { name: 'P-256', size: 32 },
  '1.3.132.0.34': { name: 'P-384', size: 48 },
};

export const notificationBodySchema = z.object({ signedPayload: z.string().min(1) });

// ── Minimal DER reader: just enough of X.509 to check a signature chain ─────

type Der = { tag: number; start: number; contentStart: number; end: number };

function readDer(bytes: Uint8Array, offset: number): Der {
  const tag = bytes[offset];
  let length = bytes[offset + 1];
  let contentStart = offset + 2;
  if (length & 0x80) {
    const lengthBytes = length & 0x7f;
    if (lengthBytes < 1 || lengthBytes > 4) {
      throw new Error('Unsupported DER length');
    }
    length = 0;
    for (let i = 0; i < lengthBytes; i++) {
      length = length * 256 + bytes[contentStart + i];
    }
    contentStart += lengthBytes;
  }
  const end = contentStart + length;
  if (tag === undefined || end > bytes.length) {
    throw new Error('Truncated DER');
  }
  return { tag, start: offset, contentStart, end };
}

function children(bytes: Uint8Array, parent: Der) {
  const items: Der[] = [];
  for (let offset = parent.contentStart; offset < parent.end; ) {
    const item = readDer(bytes, offset);
    items.push(item);
    offset = item.end;
  }
  return items;
}

function oidString(bytes: Uint8Array, item: Der) {
  const content = bytes.subarray(item.contentStart, item.end);
  const parts = [Math.floor(content[0] / 40), content[0] % 40];
  let value = 0;
  for (const byte of content.subarray(1)) {
    value = value * 128 + (byte & 0x7f);
    if (!(byte & 0x80)) {
      parts.push(value);
      value = 0;
    }
  }
  return parts.join('.');
}

function derTime(bytes: Uint8Array, item: Der) {
  const text = new TextDecoder().decode(bytes.subarray(item.contentStart, item.end));
  // UTCTime is YYMMDDHHMMSSZ; GeneralizedTime is YYYYMMDDHHMMSSZ.
  const full = item.tag === 0x17 ? `${Number(text.slice(0, 2)) >= 50 ? '19' : '20'}${text}` : text;
  const match = /^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})Z$/.exec(full);
  if (!match) {
    throw new Error('Unsupported certificate time');
  }
  const [, y, mo, d, h, mi, s] = match.map(Number);
  return Date.UTC(y, mo - 1, d, h, mi, s);
}

type Certificate = {
  der: Uint8Array;
  tbs: Uint8Array;
  signatureHash: 'SHA-256' | 'SHA-384';
  signature: Uint8Array;
  spki: Uint8Array;
  curve: { name: 'P-256' | 'P-384'; size: number };
  notBefore: number;
  notAfter: number;
  extensionOids: Set<string>;
};

function parseCertificate(der: Uint8Array): Certificate {
  const cert = readDer(der, 0);
  const [tbsItem, algItem, signatureItem] = children(der, cert);
  const signatureHash = ECDSA_HASH_BY_OID[oidString(der, children(der, algItem)[0])];
  if (!signatureHash || signatureItem.tag !== 0x03) {
    throw new Error('Unsupported certificate signature');
  }

  const tbsFields = children(der, tbsItem);
  // The version is an optional explicit [0]; everything after it is positional.
  const fields = tbsFields[0].tag === 0xa0 ? tbsFields.slice(1) : tbsFields;
  const [, , , validityItem, , spkiItem, ...rest] = fields;
  const [notBeforeItem, notAfterItem] = children(der, validityItem);
  const curveOid = oidString(der, children(der, children(der, spkiItem)[0])[1]);
  const curve = CURVE_BY_OID[curveOid];
  if (!curve) {
    throw new Error('Unsupported certificate key');
  }

  const extensionOids = new Set<string>();
  const extensionsWrapper = rest.find((item) => item.tag === 0xa3);
  if (extensionsWrapper) {
    const extensions = children(der, children(der, extensionsWrapper)[0]);
    for (const extension of extensions) {
      extensionOids.add(oidString(der, children(der, extension)[0]));
    }
  }

  return {
    der,
    tbs: der.subarray(tbsItem.start, tbsItem.end),
    signatureHash,
    // Skip the BIT STRING's unused-bits byte.
    signature: der.subarray(signatureItem.contentStart + 1, signatureItem.end),
    spki: der.subarray(spkiItem.start, spkiItem.end),
    curve,
    notBefore: derTime(der, notBeforeItem),
    notAfter: derTime(der, notAfterItem),
    extensionOids,
  };
}

/** X.509 carries ECDSA signatures as DER `SEQUENCE { r, s }`; WebCrypto wants raw r||s. */
function derSignatureToRaw(signature: Uint8Array, size: number) {
  const sequence = readDer(signature, 0);
  const raw = new Uint8Array(size * 2);
  children(signature, sequence).forEach((integer, index) => {
    let value = signature.subarray(integer.contentStart, integer.end);
    while (value.length > size && value[0] === 0) {
      value = value.subarray(1);
    }
    if (value.length > size) {
      throw new Error('Bad ECDSA signature');
    }
    raw.set(value, index * size + (size - value.length));
  });
  return raw;
}

function importKey(cert: Certificate) {
  return crypto.subtle.importKey(
    'spki',
    cert.spki,
    { name: 'ECDSA', namedCurve: cert.curve.name },
    false,
    ['verify'],
  );
}

async function isSignedBy(cert: Certificate, issuer: Certificate) {
  return crypto.subtle.verify(
    { name: 'ECDSA', hash: cert.signatureHash },
    await importKey(issuer),
    derSignatureToRaw(cert.signature, issuer.curve.size),
    cert.tbs,
  );
}

// ── JWS ─────────────────────────────────────────────────────────────────────

function base64UrlDecode(value: string) {
  const base64 = value.replace(/-/g, '+').replace(/_/g, '/');
  return Uint8Array.from(atob(base64 + '='.repeat((4 - (base64.length % 4)) % 4)), (char) =>
    char.charCodeAt(0),
  );
}

async function sha256Hex(bytes: Uint8Array) {
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

/**
 * API-057: the decoded payload of an App Store-signed JWS, or null unless its
 * x5c chain ends at the pinned root and every check passes. `rootSha256`
 * exists so tests can pin their own root; production uses Apple's.
 */
export async function verifyAppStoreJws(
  jws: string,
  rootSha256: string = APPLE_ROOT_CA_G3_SHA256,
): Promise<Record<string, unknown> | null> {
  try {
    const [encodedHeader, encodedPayload, encodedSignature, ...extra] = jws.split('.');
    if (!encodedSignature || extra.length > 0) {
      return null;
    }

    const header = JSON.parse(new TextDecoder().decode(base64UrlDecode(encodedHeader))) as {
      alg?: unknown;
      x5c?: unknown;
    };
    if (
      header.alg !== 'ES256' ||
      !Array.isArray(header.x5c) ||
      header.x5c.length !== 3 ||
      !header.x5c.every((entry) => typeof entry === 'string')
    ) {
      return null;
    }

    const ders = (header.x5c as string[]).map((entry) =>
      Uint8Array.from(atob(entry), (char) => char.charCodeAt(0)),
    );
    if ((await sha256Hex(ders[2])) !== rootSha256.toLowerCase()) {
      return null;
    }

    const [leaf, intermediate, root] = ders.map(parseCertificate);
    if (
      !leaf.extensionOids.has(LEAF_MARKER_OID) ||
      !intermediate.extensionOids.has(INTERMEDIATE_MARKER_OID) ||
      leaf.curve.name !== 'P-256'
    ) {
      return null;
    }

    const payload = JSON.parse(new TextDecoder().decode(base64UrlDecode(encodedPayload))) as Record<
      string,
      unknown
    >;
    const signedDate = payload.signedDate;
    if (typeof signedDate !== 'number') {
      return null;
    }
    for (const cert of [leaf, intermediate, root]) {
      if (signedDate < cert.notBefore || signedDate > cert.notAfter) {
        return null;
      }
    }

    if (!(await isSignedBy(leaf, intermediate)) || !(await isSignedBy(intermediate, root))) {
      return null;
    }

    const signatureValid = await crypto.subtle.verify(
      { name: 'ECDSA', hash: 'SHA-256' },
      await importKey(leaf),
      base64UrlDecode(encodedSignature),
      new TextEncoder().encode(`${encodedHeader}.${encodedPayload}`),
    );
    return signatureValid ? payload : null;
  } catch (error) {
    console.error('App Store JWS verification failed', error);
    return null;
  }
}
