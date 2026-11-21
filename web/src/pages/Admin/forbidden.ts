// A non-admin gets 403 from every admin route (API-051). The page already says
// so once, so each section stays quiet about it.
export function isForbidden(err: unknown) {
  return (
    typeof err === 'object' &&
    err !== null &&
    'status' in err &&
    (err as { status?: unknown }).status === 403
  );
}
