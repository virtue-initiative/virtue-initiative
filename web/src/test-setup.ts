import 'fake-indexeddb/auto';
// Node does not implement the Uint8Array base64/hex methods yet, so the tests
// need the same polyfill the app ships to browsers that lack them.
import './utils/uint8array-base64';
import { expect, afterAll, afterEach, beforeAll } from 'vitest';
import * as matchers from '@testing-library/jest-dom/matchers';
import { server } from './mocks/server';

expect.extend(matchers);

beforeAll(() => server.listen({ onUnhandledRequest: 'warn' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());
