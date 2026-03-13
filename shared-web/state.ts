export interface SharedState {
  theme?: string;
}

const THEME_COOKIE_NAME = "virtue-theme";
const THEME_COOKIE_MAX_AGE_SECONDS = 60 * 60 * 24 * 365;

function normalizeTheme(theme: unknown): string | undefined {
  return theme === "dark" || theme === "light" ? theme : undefined;
}

function normalizeState(nextState: SharedState): SharedState {
  const normalizedState = { ...nextState };
  const theme = normalizeTheme(nextState.theme);

  if (theme === undefined) {
    delete normalizedState.theme;
    return normalizedState;
  }

  normalizedState.theme = theme;
  return normalizedState;
}

function parseState(raw: string | null): SharedState {
  if (!raw) return {};

  try {
    return normalizeState(JSON.parse(raw) as SharedState);
  } catch {
    return {};
  }
}

function getThemeCookieDomain(hostname: string) {
  if (
    hostname === "virtueinitiative.org" ||
    hostname.endsWith(".virtueinitiative.org")
  ) {
    return ".virtueinitiative.org";
  }

  return undefined;
}

function readThemeCookie() {
  if (typeof document === "undefined") return undefined;

  const match = document.cookie.match(
    new RegExp(`(?:^|; )${THEME_COOKIE_NAME}=([^;]*)`),
  );
  return normalizeTheme(match ? decodeURIComponent(match[1]) : undefined);
}

function syncThemeCookie(theme: unknown) {
  if (typeof document === "undefined" || typeof window === "undefined") return;

  const attributes = ["Path=/", "SameSite=Lax"];
  const domain = getThemeCookieDomain(window.location.hostname);

  if (domain) {
    attributes.push(`Domain=${domain}`);
  }

  if (window.location.protocol === "https:") {
    attributes.push("Secure");
  }

  const normalizedTheme = normalizeTheme(theme);

  if (normalizedTheme === undefined) {
    document.cookie = `${THEME_COOKIE_NAME}=; ${attributes.join("; ")}; Max-Age=0`;
    return;
  }

  document.cookie =
    `${THEME_COOKIE_NAME}=${encodeURIComponent(normalizedTheme)}; ` +
    `${attributes.join("; ")}; Max-Age=${THEME_COOKIE_MAX_AGE_SECONDS}`;
}

function loadState(storageKey: string) {
  if (typeof window === "undefined") return {};

  const state = parseState(localStorage.getItem(storageKey));
  const cookieTheme = readThemeCookie();

  if (state.theme !== undefined || cookieTheme === undefined) {
    return state;
  }

  return { ...state, theme: cookieTheme };
}

const clients = new Set<{ source: MessageEventSource; origin: string }>();
let state = typeof window === "undefined" ? {} : loadState("shared-state");
let initialized = false;
let server = (
  typeof window === "undefined" ? undefined : document.createElement("iframe")
)!;

function updateServerLocalState(newState: SharedState) {
  state = normalizeState({ ...state, ...newState });
  localStorage.setItem("shared-state", JSON.stringify(state));
  syncThemeCookie(state.theme);
}

function handleStateChange(newState: SharedState) {
  clients.forEach((client) => {
    client.source.postMessage(
      { type: "state-change", state: newState },
      {
        targetOrigin: client.origin,
      },
    );
  });
}

export function startStateServer() {
  console.log("Starting shared state server", state);

  state = loadState("shared-state");
  syncThemeCookie(state.theme);

  window.addEventListener("storage", (event) => {
    if (event.key === "shared-state") {
      state = parseState(event.newValue);
      syncThemeCookie(state.theme);
      handleStateChange(state);
    }
  });

  window.addEventListener("message", (event) => {
    const data = event.data;
    if (!data || typeof data !== "object") return;

    if (data.type === "state-update") {
      updateServerLocalState(data.state);
      handleStateChange(data.state);
    } else if (data.type === "state-register") {
      if (!event.source) return;

      clients.add({ source: event.source, origin: event.origin });

      event.source.postMessage(
        { type: "state-change", state },
        {
          targetOrigin: event.origin,
        },
      );
    }
  });
}

// Client code
let clientState =
  typeof window === "undefined" ? {} : loadState("shared-state-local");

function getServerLocation() {
  if (window.location.hostname === "localhost") {
    return "http://localhost:4321/state-iframe.html";
  } else {
    return "https://virtueinitiative.org/state-iframe.html";
  }
}

export function onStateUpdate(callback: (state: SharedState) => void) {
  if (!initialized) {
    initialized = true;
    syncThemeCookie(clientState.theme);
    server.style.display = "none";
    server.hidden = true;
    server.src = getServerLocation();
    document.body.appendChild(server);
    server.addEventListener("load", () => {
      server.contentWindow?.postMessage(
        { type: "state-register" },
        getServerLocation(),
      );
    });
  }

  callback(clientState);

  window.addEventListener("message", (event) => {
    const data = event.data;
    if (!data || typeof data !== "object") return;

    if (data.type === "state-change") {
      clientState = normalizeState(data.state as SharedState);
      localStorage.setItem("shared-state-local", JSON.stringify(clientState));
      syncThemeCookie(clientState.theme);
      callback(clientState);
    }
  });
}

export function updateState(newState: Partial<SharedState>) {
  clientState = normalizeState({ ...clientState, ...newState });
  localStorage.setItem("shared-state-local", JSON.stringify(clientState));
  syncThemeCookie(clientState.theme);
  server.contentWindow?.postMessage(
    { type: "state-update", state: newState },
    getServerLocation(),
  );
}

export function getState() {
  return clientState;
}

// Theme specific code

export function preferredTheme() {
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export function nextTheme() {
  const theme = getState().theme;
  let next = undefined as string | undefined;
  if (theme === undefined) {
    next = preferredTheme() === "dark" ? "light" : "dark";
  } else if (theme === "dark") {
    next = preferredTheme() === "light" ? undefined : "light";
  } else if (theme === "light") {
    next = preferredTheme() === "dark" ? undefined : "dark";
  }

  updateState({ theme: next });
}
