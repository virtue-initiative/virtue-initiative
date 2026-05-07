import useSWR, { useSWRConfig } from "swr";
import { api, Device, isToastHandledError } from "../api";
import {
  removeDeviceFromCachedDataFeed,
  deleteDecryptedEventsForDevice,
} from "../data-cache";
import { useAuth } from "../context/auth";
import { isLogsKey, swrKeys } from "./swr-keys";

function requireToken(token: string | null): string {
  if (!token) {
    throw new Error("You must be logged in to perform this action.");
  }

  return token;
}

export interface UseDevicesResult {
  devices: Device[] | undefined;
  error: Error | undefined;
  isLoading: boolean;
  updateDevice: (id: string, patch: { name?: string }) => Promise<void>;
  removeDevice: (id: string) => Promise<void>;
}

export function useDevices(): UseDevicesResult {
  const { token, userId } = useAuth();
  const { mutate } = useSWRConfig();
  const key = token ? swrKeys.devices(token) : null;
  const { data, error, isLoading } = useSWR<Device[], Error>(key, () =>
    api.getDevices(requireToken(token)),
  );

  const updateDevice = async (id: string, patch: { name?: string }) => {
    const authToken = requireToken(token);
    await api.patchDevice(authToken, id, patch);
    await mutate(swrKeys.devices(authToken));
  };

  const removeDevice = async (id: string) => {
    const authToken = requireToken(token);
    await api.deleteDevice(authToken, id);

    if (userId) {
      await removeDeviceFromCachedDataFeed(userId, userId, id).catch((err) => {
        console.warn(
          "[devices] failed to remove deleted device from cache",
          err,
        );
      });
      await deleteDecryptedEventsForDevice(userId, id).catch((err) => {
        console.warn(
          "[devices] failed to wipe decrypted events for device",
          err,
        );
      });
    }

    await Promise.all([
      mutate(swrKeys.devices(authToken)),
      mutate((cacheKey) => isLogsKey(cacheKey), undefined, {
        revalidate: true,
      }),
    ]);
  };

  return {
    devices: data,
    error: error && !isToastHandledError(error) ? error : undefined,
    isLoading: Boolean(token) && isLoading,
    updateDevice,
    removeDevice,
  };
}
