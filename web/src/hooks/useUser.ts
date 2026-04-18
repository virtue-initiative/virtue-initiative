import useSWR, { useSWRConfig } from "swr";
import { api, User } from "../api";
import { useAuth } from "../context/auth";
import { swrKeys } from "./swr-keys";

function requireToken(token: string | null): string {
  if (!token) {
    throw new Error("You must be logged in to perform this action.");
  }

  return token;
}

export interface UseUserResult {
  user: User | undefined;
  error: Error | undefined;
  isLoading: boolean;
  updateUser: (patch: {
    email?: string;
    name?: string;
    email_frequency?: User["email_frequency"];
    email_digest_minutes_utc?: User["email_digest_minutes_utc"];
    pub_key?: string;
    priv_key?: string;
  }) => Promise<{
    email_verification_required?: boolean;
    pending_email?: string;
  }>;
  deleteUser: (confirmEmail: string) => Promise<void>;
}

export function useUser(): UseUserResult {
  const { token } = useAuth();
  const { mutate } = useSWRConfig();
  const key = token ? swrKeys.user(token) : null;
  const { data, error, isLoading } = useSWR<User, Error>(key, () =>
    api.getUser(requireToken(token)),
  );

  const updateUser = async (
    patch: Parameters<typeof api.updateUser>[1],
  ): Promise<{
    email_verification_required?: boolean;
    pending_email?: string;
  }> => {
    const authToken = requireToken(token);
    const result = await api.updateUser(authToken, patch);
    await mutate(swrKeys.user(authToken));
    return {
      email_verification_required: result.email_verification_required,
      pending_email: result.pending_email,
    };
  };

  const deleteUser = async (confirmEmail: string) => {
    const authToken = requireToken(token);
    await api.deleteUser(authToken, confirmEmail);
    await mutate(swrKeys.user(authToken), undefined, {
      revalidate: false,
    });
  };

  return {
    user: data,
    error,
    isLoading: Boolean(token) && isLoading,
    updateUser,
    deleteUser,
  };
}
