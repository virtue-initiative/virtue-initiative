import { cacheClient } from '../cache/client';

export async function loadEventImage(
  _viewerId: string,
  eventId: string,
): Promise<Uint8Array | undefined> {
  const result = await cacheClient?.getEventImage(eventId);
  return result ?? undefined;
}
