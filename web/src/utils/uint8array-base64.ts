/**
 * Polyfill for the Uint8Array base64/hex conversion methods (the TC39
 * "Uint8Array from/to base64" proposal), declared in `src/globals.d.ts` and
 * used throughout the auth and crypto code.
 *
 * They ship in Firefox 133+, Safari 18.2+, and Chrome 140+, so browsers that
 * are otherwise current can still be missing them. Import this module for its
 * side effect before any code that converts a Uint8Array, from both the app
 * entry point and the cache worker. Installing is a no-op where the engine
 * already provides them.
 */

const BASE64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
const BASE64URL_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';

// btoa/atob take strings, and spreading a large array into String.fromCharCode
// overflows the call stack, so convert in chunks.
const CHUNK_SIZE = 0x8000;

type Base64Options = {
  alphabet?: 'base64' | 'base64url';
  omitPadding?: boolean;
};

function bytesToBinaryString(bytes: Uint8Array): string {
  let result = '';
  for (let i = 0; i < bytes.length; i += CHUNK_SIZE) {
    result += String.fromCharCode(...bytes.subarray(i, i + CHUNK_SIZE));
  }
  return result;
}

function encodeBase64(bytes: Uint8Array, options?: Base64Options): string {
  let encoded = btoa(bytesToBinaryString(bytes));
  if (options?.alphabet === 'base64url') {
    encoded = encoded.replace(/\+/g, '-').replace(/\//g, '_');
  }
  if (options?.omitPadding) {
    encoded = encoded.replace(/=+$/, '');
  }
  return encoded;
}

function decodeBase64(value: string, options?: Base64Options): Uint8Array<ArrayBuffer> {
  const alphabet = options?.alphabet === 'base64url' ? BASE64URL_ALPHABET : BASE64_ALPHABET;
  let normalized = value;
  if (options?.alphabet === 'base64url') {
    normalized = normalized.replace(/-/g, '+').replace(/_/g, '/');
  }
  // atob rejects unpadded input, which the spec accepts.
  const remainder = normalized.length % 4;
  if (remainder === 2 || remainder === 3) {
    normalized += '='.repeat(4 - remainder);
  }
  for (const char of value) {
    if (char !== '=' && !alphabet.includes(char)) {
      throw new SyntaxError(`Invalid base64 character: ${char}`);
    }
  }
  return Uint8Array.from(atob(normalized), (char) => char.charCodeAt(0));
}

function encodeHex(bytes: Uint8Array): string {
  let result = '';
  for (const byte of bytes) {
    result += byte.toString(16).padStart(2, '0');
  }
  return result;
}

function decodeHex(value: string): Uint8Array<ArrayBuffer> {
  if (value.length % 2 !== 0) {
    throw new SyntaxError('Hex string must have an even number of characters');
  }
  // Number.parseInt would accept '1z' as 1, so check the whole string first.
  if (!/^[0-9a-fA-F]*$/.test(value)) {
    throw new SyntaxError('Hex string contains a non-hex character');
  }
  const bytes = new Uint8Array(value.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = Number.parseInt(value.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

/**
 * Copies as many bytes as fit into `target`, per setFromBase64/setFromHex, and
 * reports how many source characters those bytes came from.
 */
function setFrom(target: Uint8Array, source: Uint8Array, readFor: (written: number) => number) {
  const written = Math.min(target.length, source.length);
  target.set(source.subarray(0, written));
  return { read: readFor(written), written };
}

/** Every 3 bytes come from 4 base64 characters; 1 and 2 leftover bytes take 2 and 3. */
function base64CharsFor(byteCount: number): number {
  const remainder = byteCount % 3;
  return Math.floor(byteCount / 3) * 4 + (remainder === 0 ? 0 : remainder + 1);
}

export function installUint8ArrayBase64Polyfill() {
  if (typeof Uint8Array.prototype.toBase64 !== 'function') {
    Uint8Array.prototype.toBase64 = function toBase64(options?: Base64Options) {
      return encodeBase64(this, options);
    };
  }

  if (typeof Uint8Array.prototype.setFromBase64 !== 'function') {
    Uint8Array.prototype.setFromBase64 = function setFromBase64(
      value: string,
      options?: Base64Options,
    ) {
      return setFrom(this, decodeBase64(value, options), base64CharsFor);
    };
  }

  if (typeof Uint8Array.prototype.toHex !== 'function') {
    Uint8Array.prototype.toHex = function toHex() {
      return encodeHex(this);
    };
  }

  if (typeof Uint8Array.prototype.setFromHex !== 'function') {
    Uint8Array.prototype.setFromHex = function setFromHex(value: string) {
      return setFrom(this, decodeHex(value), (written) => written * 2);
    };
  }

  if (typeof Uint8Array.fromBase64 !== 'function') {
    Uint8Array.fromBase64 = (value: string, options?: Base64Options) =>
      decodeBase64(value, options);
  }

  if (typeof Uint8Array.fromHex !== 'function') {
    Uint8Array.fromHex = (value: string) => decodeHex(value);
  }
}

installUint8ArrayBase64Polyfill();
