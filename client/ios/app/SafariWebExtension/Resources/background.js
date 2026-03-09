const CAPTURE_MIN_INTERVAL_MS = 3000;
let lastCaptureAttemptAt = 0;

function maybeBrowser() {
  if (typeof browser !== "undefined") {
    return browser;
  }
  return null;
}

function isHttpPage(url) {
  return typeof url === "string" && /^https?:\/\//i.test(url);
}

async function sendNative(payload) {
  const b = maybeBrowser();
  if (!b || !b.runtime || typeof b.runtime.sendNativeMessage !== "function") {
    return;
  }

  try {
    await b.runtime.sendNativeMessage(payload);
    return;
  } catch (_) {
    // Safari/WebExtension runtime signatures vary; try a host argument fallback.
  }

  const hostCandidate = b.runtime.id || "native";
  await b.runtime.sendNativeMessage(hostCandidate, payload);
}

async function activeTabFallback() {
  const b = maybeBrowser();
  if (!b || !b.tabs || typeof b.tabs.query !== "function") {
    return null;
  }
  const tabs = await b.tabs.query({ active: true, lastFocusedWindow: true });
  if (!Array.isArray(tabs) || tabs.length === 0) {
    return null;
  }
  return tabs[0];
}

async function captureAndSend(tab, source) {
  const b = maybeBrowser();
  if (!b || !b.tabs || typeof b.tabs.captureVisibleTab !== "function") {
    await sendNative({
      type: "capture_error",
      error: "tabs.captureVisibleTab unavailable",
      source
    }).catch(() => {});
    return { ok: false, error: "capture_api_unavailable" };
  }

  const resolvedTab = tab || (await activeTabFallback());
  if (!resolvedTab || !isHttpPage(resolvedTab.url)) {
    await sendNative({ type: "ping", source }).catch(() => {});
    return { ok: true, skipped: true, reason: "non_http_tab" };
  }

  const now = Date.now();
  if (now - lastCaptureAttemptAt < CAPTURE_MIN_INTERVAL_MS) {
    return { ok: true, skipped: true, reason: "throttled" };
  }
  lastCaptureAttemptAt = now;

  try {
    const dataUrl = await b.tabs.captureVisibleTab(resolvedTab.windowId, { format: "png" });
    await sendNative({
      type: "capture_frame",
      png_data_url: dataUrl,
      url: resolvedTab.url || "",
      title: resolvedTab.title || "",
      captured_at_ms: now,
      source
    });
    return { ok: true };
  } catch (error) {
    const message = error && error.message ? String(error.message) : String(error);
    await sendNative({
      type: "capture_error",
      error: message,
      url: resolvedTab.url || "",
      source
    }).catch(() => {});
    return { ok: false, error: message };
  }
}

const b = maybeBrowser();

if (b && b.runtime && typeof b.runtime.onInstalled?.addListener === "function") {
  b.runtime.onInstalled.addListener(() => {
    sendNative({ type: "ping", source: "installed" }).catch(() => {});
  });
}

if (b && b.runtime && typeof b.runtime.onStartup?.addListener === "function") {
  b.runtime.onStartup.addListener(() => {
    sendNative({ type: "ping", source: "startup" }).catch(() => {});
  });
}

if (b && b.runtime && typeof b.runtime.onMessage?.addListener === "function") {
  b.runtime.onMessage.addListener((message, sender) => {
    if (!message || message.type !== "virtue_capture_tick") {
      return undefined;
    }
    return captureAndSend(sender?.tab || null, "content_tick");
  });
}

if (b && b.tabs && typeof b.tabs.onActivated?.addListener === "function") {
  b.tabs.onActivated.addListener(async ({ tabId }) => {
    try {
      const tab = await b.tabs.get(tabId);
      await captureAndSend(tab, "tab_activated");
    } catch (_) {
      // ignore best-effort capture for tab activation
    }
  });
}

if (b && b.tabs && typeof b.tabs.onUpdated?.addListener === "function") {
  b.tabs.onUpdated.addListener((_, changeInfo, tab) => {
    if (!tab || !tab.active) {
      return;
    }
    if (changeInfo.status === "complete" || typeof changeInfo.url === "string") {
      captureAndSend(tab, "tab_updated").catch(() => {});
    }
  });
}
