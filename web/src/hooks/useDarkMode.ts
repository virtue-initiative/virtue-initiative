import { useEffect, useState } from "preact/hooks";
import {
  getState,
  nextTheme,
  onStateUpdate,
  preferredTheme,
} from "@virtueinitiative/shared-web/state";

export function useDarkMode() {
  const [dark, setDark] = useState(() => {
    if (typeof window === "undefined") return false;

    return (getState().theme ?? preferredTheme()) === "dark";
  });

  useEffect(() => {
    if (typeof window === "undefined") return;

    onStateUpdate((state) => {
      console.log("Initializing theme...", state);
      document.documentElement.setAttribute("data-theme", state.theme);
      setDark((state.theme ?? preferredTheme()) === "dark");
    });
  }, []);

  return { dark, toggle: nextTheme };
}
