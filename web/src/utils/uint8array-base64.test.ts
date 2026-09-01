import { describe, expect, it } from 'vitest';
import './uint8array-base64';

// Node does not implement these natively, so in this suite they are the polyfill.
const BYTES = new Uint8Array([0x00, 0x01, 0x7f, 0x80, 0xff, 0xfe]);

describe('toBase64', () => {
  it('round-trips through fromBase64', () => {
    expect(Uint8Array.fromBase64(BYTES.toBase64())).toEqual(BYTES);
  });

  it('matches btoa for the standard alphabet', () => {
    expect(BYTES.toBase64()).toBe('AAF/gP/+');
  });

  it('pads by default and omits padding on request', () => {
    const bytes = new Uint8Array([1, 2, 3, 4]);
    expect(bytes.toBase64()).toBe('AQIDBA==');
    expect(bytes.toBase64({ omitPadding: true })).toBe('AQIDBA');
  });

  it('uses the URL-safe alphabet when asked', () => {
    expect(BYTES.toBase64({ alphabet: 'base64url' })).toBe('AAF_gP_-');
  });

  it('handles an input larger than one conversion chunk', () => {
    const big = new Uint8Array(200_000).map((_, i) => i % 256);
    expect(Uint8Array.fromBase64(big.toBase64())).toEqual(big);
  });

  it('encodes an empty array as an empty string', () => {
    expect(new Uint8Array(0).toBase64()).toBe('');
  });
});

describe('fromBase64', () => {
  it('accepts unpadded input', () => {
    expect(Uint8Array.fromBase64('AQIDBA')).toEqual(new Uint8Array([1, 2, 3, 4]));
  });

  it('accepts the URL-safe alphabet', () => {
    expect(Uint8Array.fromBase64('AAF_gP_-', { alphabet: 'base64url' })).toEqual(BYTES);
  });

  it('throws on a character outside the alphabet', () => {
    expect(() => Uint8Array.fromBase64('AA*A')).toThrow(SyntaxError);
  });
});

describe('setFromBase64', () => {
  it('fills the target and reports what it read', () => {
    const target = new Uint8Array(4);
    expect(target.setFromBase64('AQIDBA==')).toEqual({ read: 6, written: 4 });
    expect(target).toEqual(new Uint8Array([1, 2, 3, 4]));
  });

  it('stops when the target runs out of room', () => {
    const target = new Uint8Array(3);
    expect(target.setFromBase64('AQIDBA==')).toEqual({ read: 4, written: 3 });
    expect(target).toEqual(new Uint8Array([1, 2, 3]));
  });
});

describe('toHex and fromHex', () => {
  it('round-trips', () => {
    expect(BYTES.toHex()).toBe('00017f80fffe');
    expect(Uint8Array.fromHex('00017f80fffe')).toEqual(BYTES);
  });

  it('throws on an odd-length string', () => {
    expect(() => Uint8Array.fromHex('abc')).toThrow(SyntaxError);
  });

  it('throws on a non-hex character', () => {
    expect(() => Uint8Array.fromHex('zz')).toThrow(SyntaxError);
    expect(() => Uint8Array.fromHex('1z')).toThrow(SyntaxError);
  });
});

describe('setFromHex', () => {
  it('fills the target and reports what it read', () => {
    const target = new Uint8Array(2);
    expect(target.setFromHex('0102')).toEqual({ read: 4, written: 2 });
    expect(target).toEqual(new Uint8Array([1, 2]));
  });
});
