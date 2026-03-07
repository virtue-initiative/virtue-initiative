import React, {useEffect, useState} from "react";
import {useColorMode} from "@docusaurus/theme-common";
import {
  getState,
  onStateUpdate,
  nextTheme,
  preferredTheme,
} from "@virtueinitiative/shared-web/state";

function MoonIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{width:"1.1rem", height: "1.1rem"}}>
      <path d="M21.752 15.002A9.718 9.718 0 0 1 18 15.75 9.75 9.75 0 0 1 8.25 6c0-1.33.266-2.596.748-3.752a9.75 9.75 0 1 0 12.754 12.754Z" />
    </svg>
  );
}

function SunIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{width:"1.1rem", height: "1.1rem"}}>
      <path d="M12 3v2.25M12 18.75V21M5.636 5.636l1.591 1.591M16.773 16.773l1.591 1.591M3 12h2.25M18.75 12H21M5.636 18.364l1.591-1.591M16.773 7.227l1.591-1.591M15.75 12a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0Z" />
    </svg>
  );
}

export default function ColorModeToggle() {
  const {setColorMode} = useColorMode();
  const [theme, setTheme] = useState<string | undefined>(getState().theme);

  function applyTheme(t: string | undefined) {
    const effective = t ?? preferredTheme();
    setColorMode(effective as "dark" | "light");
  }

  useEffect(() => {
    applyTheme(theme);
  }, []);

  useEffect(() => {
    const unsubscribe = onStateUpdate((state) => {
      setTheme(state.theme);
      applyTheme(state.theme);
    });

    return () => {
      if (typeof unsubscribe === "function") unsubscribe();
    };
  }, [setColorMode]);

  const effectiveTheme = theme ?? preferredTheme();
  const isDark = effectiveTheme === "dark";

  return (
    <button
      type="button"
      className="btn-icon"
      aria-label="Toggle color mode"
      title="Toggle color mode"
      onClick={() => nextTheme()}
          >
      {isDark ? <SunIcon /> : <MoonIcon />}
    </button>
  );
}

