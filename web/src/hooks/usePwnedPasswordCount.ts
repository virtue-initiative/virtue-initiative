import { useEffect, useState } from 'preact/hooks';
import { checkPwnedPassword, passwordLengthError } from '../utils/password-policy';

/**
 * Debounced breach lookup for a password the user is choosing. Returns the
 * breach count, or null while typing, when disabled, or when the lookup fails.
 */
export function usePwnedPasswordCount(password: string, enabled: boolean): number | null {
  const [pwnedCount, setPwnedCount] = useState<number | null>(null);

  useEffect(() => {
    setPwnedCount(null);
    // Wait until the password passes the length rule so a half-typed password
    // is not looked up on every keystroke.
    if (!enabled || passwordLengthError(password)) return;

    const controller = new AbortController();
    const timer = setTimeout(() => {
      checkPwnedPassword(password, controller.signal).then((count) => {
        if (!controller.signal.aborted) setPwnedCount(count);
      });
    }, 400);

    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [password, enabled]);

  return pwnedCount;
}
