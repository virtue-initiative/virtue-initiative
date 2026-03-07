import {
  onStateUpdate,
  nextTheme,
  preferredTheme,
  getState,
} from "@virtueinitiative/shared-web/state.ts";

let MOON_SVG =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" style="width:1.1rem;height:1.1rem"><path d="M21.752 15.002A9.718 9.718 0 0 1 18 15.75 9.75 9.75 0 0 1 8.25 6c0-1.33.266-2.596.748-3.752a9.75 9.75 0 1 0 12.754 12.754Z"/></svg>';
let SUN_SVG =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" style="width:1.1rem;height:1.1rem"><path d="M12 3v2.25M12 18.75V21M5.636 5.636l1.591 1.591M16.773 16.773l1.591 1.591M3 12h2.25M18.75 12H21M5.636 18.364l1.591-1.591M16.773 7.227l1.591-1.591M15.75 12a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0Z"/></svg>';

document.documentElement.setAttribute("data-theme", getState().theme ?? "");

const onReady = () => {
  document.getElementById("theme-toggle")!.addEventListener("click", nextTheme);

  onStateUpdate((state) => {
    document.documentElement.setAttribute("data-theme", state.theme ?? "");

    document.getElementById("theme-toggle")!.innerHTML =
      (state.theme ?? preferredTheme()) === "dark" ? SUN_SVG : MOON_SVG;
  });
};

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", onReady);
} else {
  onReady();
}
