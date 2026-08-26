import { describe, expect, it } from 'vitest';
import { describeError } from './api';

describe('describeError', () => {
  it('returns the message from a normal Error', () => {
    expect(describeError(new Error('boom'), 'fallback')).toBe('boom');
  });

  it('returns null for a toastHandled error (network error already surfaced)', () => {
    const err = Object.assign(new Error(''), { toastHandled: true });
    expect(describeError(err, 'fallback')).toBeNull();
  });

  it('returns the fallback for a non-Error throw', () => {
    expect(describeError('not an error', 'fallback')).toBe('fallback');
  });

  it('returns the fallback for an Error with an empty message', () => {
    expect(describeError(new Error(''), 'fallback')).toBe('fallback');
  });
});
