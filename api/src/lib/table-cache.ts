// An in-memory copy of a small table, per isolate, kept for up to 60 seconds
// (API-061, API-068). An admin route that changes the table calls `clear`, so
// this isolate sees the change at once.
const CACHE_TTL_MS = 60_000;

export function tableCache<T>(load: (db: D1Database) => Promise<T>) {
  let cached: { value: T; expiresAt: number } | null = null;
  return {
    async get(db: D1Database, now: number) {
      if (cached && cached.expiresAt > now) {
        return cached.value;
      }
      const value = await load(db);
      cached = { value, expiresAt: now + CACHE_TTL_MS };
      return value;
    },
    clear() {
      cached = null;
    },
  };
}
