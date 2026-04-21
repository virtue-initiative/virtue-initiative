import useSWR, { useSWRConfig } from "swr";
import {
  api,
  isToastHandledError,
  PartnerRelationships,
  WatchingPartner,
  WatcherPartner,
} from "../api";
import { useAuth } from "../context/auth";
import { isLogsKey, swrKeys } from "./swr-keys";

function requireToken(token: string | null): string {
  if (!token) {
    throw new Error("You must be logged in to perform this action.");
  }

  return token;
}

export interface UsePartnersResult {
  partners: PartnerRelationships | undefined;
  watching: WatchingPartner[] | undefined;
  watchers: WatcherPartner[] | undefined;
  error: Error | undefined;
  isLoading: boolean;
  invitePartner: (email: string) => Promise<void>;
  acceptPartnerInvite: (inviteToken: string) => Promise<void>;
  removeWatching: (id: string) => Promise<void>;
  removeWatcher: (id: string) => Promise<void>;
}

export function usePartners(): UsePartnersResult {
  const { token } = useAuth();
  const { mutate } = useSWRConfig();
  const key = token ? swrKeys.partners(token) : null;
  const { data, error, isLoading } = useSWR<PartnerRelationships, Error>(
    key,
    () => api.getPartners(requireToken(token)),
  );

  async function revalidatePartnerRelated(authToken: string) {
    await Promise.all([
      mutate(swrKeys.partners(authToken)),
      mutate(swrKeys.devices(authToken)),
      mutate((cacheKey) => isLogsKey(cacheKey), undefined, {
        revalidate: true,
      }),
    ]);
  }

  const invitePartner = async (email: string) => {
    const authToken = requireToken(token);
    await api.invitePartner(authToken, email);
    await mutate(swrKeys.partners(authToken));
  };

  const acceptPartnerInvite = async (inviteToken: string) => {
    const authToken = requireToken(token);
    await api.acceptPartnerInvite(authToken, inviteToken);
    await revalidatePartnerRelated(authToken);
  };

  const removeWatching = async (id: string) => {
    const authToken = requireToken(token);
    await api.deleteWatching(authToken, id);
    await revalidatePartnerRelated(authToken);
  };

  const removeWatcher = async (id: string) => {
    const authToken = requireToken(token);
    await api.deleteWatcher(authToken, id);
    await revalidatePartnerRelated(authToken);
  };

  return {
    partners: data,
    watching: data?.watching,
    watchers: data?.watchers,
    error: error && !isToastHandledError(error) ? error : undefined,
    isLoading: Boolean(token) && isLoading,
    invitePartner,
    acceptPartnerInvite,
    removeWatching,
    removeWatcher,
  };
}
