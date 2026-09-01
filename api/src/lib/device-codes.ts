import { randomBytes } from 'node:crypto';

/**
 * API-043: the user-code alphabet. 30 characters, with `I`, `L`, `O`, `U`, `0`
 * and `1` removed so a code read off one screen and typed into another can't be
 * misread. 30^6 is roughly 729 million codes.
 */
export const ALPHABET = '23456789ABCDEFGHJKMNPQRSTVWXYZ';
export const USER_CODE_LENGTH = 6;

/**
 * 256 is not a multiple of 30, so plain `byte % 30` would make the first 16
 * letters of the alphabet ~7% likelier than the rest. Reject the tail of the
 * byte range instead and draw again.
 */
const REJECTION_LIMIT = Math.floor(256 / ALPHABET.length) * ALPHABET.length; // 240

export function generateUserCode(): string {
  let code = '';

  while (code.length < USER_CODE_LENGTH) {
    for (const byte of randomBytes(USER_CODE_LENGTH)) {
      if (byte >= REJECTION_LIMIT) continue;
      code += ALPHABET[byte % ALPHABET.length];
      if (code.length === USER_CODE_LENGTH) break;
    }
  }

  return code;
}

/** `'K7RM3X'` -> `'K7R-M3X'`, the shape the code is displayed in. */
export function formatUserCode(code: string): string {
  return `${code.slice(0, 3)}-${code.slice(3)}`;
}

/**
 * Accepts whatever the user typed — `k7r m3x`, `K7R-M3X`, `k7rm3x` — and returns
 * the stored form, or null if it isn't a well-formed code.
 */
export function normalizeUserCode(input: string): string | null {
  const stripped = input
    .toUpperCase()
    .split('')
    .filter((char) => ALPHABET.includes(char))
    .join('');

  return stripped.length === USER_CODE_LENGTH ? stripped : null;
}
