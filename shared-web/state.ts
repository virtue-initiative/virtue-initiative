export interface SharedState {
  theme?: string;
}

const clients = new Set<{source: MessageEventSource, origin: string}>();
let state = JSON.parse(localStorage.getItem('shared-state') || '{}') as SharedState;
let initialized = false;
let server = document.createElement('iframe');

function updateServerLocalState(newState: SharedState) {
  state = {...state, ...newState};
  localStorage.setItem('shared-state', JSON.stringify(state));
}

function handleStateChange(newState: SharedState) {
  clients.forEach((client) => {
    client.source.postMessage({ type: 'state-change', state: newState }, {
      targetOrigin: client.origin,
    });
  });
}

export function startStateServer() {
  console.log("Starting shared state server", state);

  state = JSON.parse(localStorage.getItem('shared-state') || '{}') as SharedState;

  window.addEventListener('storage', (event) => {
    if (event.key === 'shared-state' && event.newValue) {
      state = JSON.parse(event.newValue) as SharedState;
      handleStateChange(state);
    }
  });

  window.addEventListener('message', (event) => {
    const data = event.data;
    if (!data || typeof data !== 'object') return;

    if (data.type === 'state-update') {
      updateServerLocalState(data.state);
      handleStateChange(data.state);
    } else if (data.type === 'state-register') {
      if (!event.source) return;

      clients.add({source: event.source, origin: event.origin});

      event.source.postMessage({ type: 'state-change', state }, {
        targetOrigin: event.origin,
      });
    }
  });
}

// Client code
let clientState = JSON.parse(localStorage.getItem('shared-state-local') || '{}') as SharedState;

function getServerLocation() {
  if (window.location.hostname === 'localhost') {
    return 'http://localhost:5174/state-iframe.html';
  } else {
    return 'https://virtueinitiative.org/state-iframe.html';
  }
}


export function onStateUpdate(callback: (state: SharedState) => void) {
  if (!initialized) {
    initialized = true;
    server.style.display = 'none';
    server.hidden = true;
    server.src = getServerLocation();
    document.body.appendChild(server);
    server.addEventListener('load', () => {
      server.contentWindow?.postMessage({ type: 'state-register' }, getServerLocation());
    });
  }

  callback(clientState);

  window.addEventListener('message', (event) => {
    const data = event.data;
    if (!data || typeof data !== 'object') return;

    if (data.type === 'state-change') {
      clientState = data.state;
      localStorage.setItem('shared-state-local', JSON.stringify(data.state));
      callback(data.state);
    }
  });
}

export function updateState(newState: Partial<SharedState>) {
  clientState = {...clientState, ...newState};
  localStorage.setItem('shared-state-local', JSON.stringify(clientState));
  server.contentWindow?.postMessage({ type: 'state-update', state: newState }, getServerLocation());
}


export function getState() {
  return clientState;
}

// Theme specific code

export function preferredTheme() {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function nextTheme() {
  const theme = getState().theme;
  let next = undefined as string | undefined;
  if (theme === undefined) {
    next = preferredTheme() === 'dark' ? 'light' : 'dark';
  } else if (theme === 'dark') {
    next = preferredTheme() === 'light' ? undefined : 'light';
  } else if (theme === 'light') {
    next = preferredTheme() === 'dark' ? undefined : 'dark';
  }

  updateState({ theme: next });
}

