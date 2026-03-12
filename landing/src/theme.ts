export const THEME_COOKIE_NAME = "virtue-theme";

export function normalizeThemeCookie(theme: string | undefined) {
  return theme === "dark" || theme === "light" ? theme : undefined;
}
