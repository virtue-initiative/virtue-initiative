// Augments vitest's Assertion interface with @testing-library/jest-dom matchers.
// The runtime side is handled by expect.extend(matchers) in test-setup.ts.
import type { TestingLibraryMatchers } from '@testing-library/jest-dom/matchers';

declare module 'vitest' {
  interface Assertion<T = any> extends TestingLibraryMatchers<T, void> {}
  interface AsymmetricMatchersContaining extends TestingLibraryMatchers<any, void> {}
}
