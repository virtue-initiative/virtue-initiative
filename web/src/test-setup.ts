import 'fake-indexeddb/auto';
import { expect, afterAll, afterEach, beforeAll } from 'vitest';
import * as matchers from '@testing-library/jest-dom/matchers';
import { server } from './mocks/server';

expect.extend(matchers);

// Node's test runtime doesn't enable Uint8Array.prototype.toBase64/fromBase64 without the
// --js-base-64 flag, even though every real browser we ship to supports it natively. Polyfill
// so tests exercising the crypto/session code paths that use these don't need that flag.
if (typeof Uint8Array.prototype.toBase64 !== 'function') {
  Uint8Array.prototype.toBase64 = function toBase64(this: Uint8Array): string {
    return Buffer.from(this).toString('base64');
  };
}
if (typeof Uint8Array.fromBase64 !== 'function') {
  Uint8Array.fromBase64 = (base64: string): Uint8Array<ArrayBuffer> => {
    const buf = Buffer.from(base64, 'base64');
    const out = new Uint8Array(new ArrayBuffer(buf.length));
    out.set(buf);
    return out;
  };
}

beforeAll(() => server.listen({ onUnhandledRequest: 'warn' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());
