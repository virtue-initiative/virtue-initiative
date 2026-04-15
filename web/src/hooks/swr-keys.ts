export const swrKeys = {
  user: (token: string) => ["user", token] as const,
  partners: (token: string) => ["partners", token] as const,
  devices: (token: string) => ["devices", token] as const,
  logs: (
    token: string,
    viewerUserId: string,
    targetUserId: string,
    deviceIdsKey: string,
  ) => ["logs", token, viewerUserId, targetUserId, deviceIdsKey] as const,
};

export function isLogsKey(key: unknown): boolean {
  return Array.isArray(key) && key[0] === "logs";
}
