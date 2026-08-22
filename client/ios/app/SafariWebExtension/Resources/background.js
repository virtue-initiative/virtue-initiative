const CAPTURE_MIN_INTERVAL_MS = 3000;
// Self-heartbeat so we get diagnostics logged even when no tab is visible/active
// (content.js's tick loop only fires for foreground tabs). This is the signal we
// watch for gaps in: if these stop appearing, either the service worker got
// evicted or the native App Extension process got jetsam-killed.
const HEARTBEAT_INTERVAL_MS = 5000;
let lastCaptureAttemptAt = 0;
let messageSeq = 0;

const swStartedAt = Date.now();
console.log(
  `[virtue] background.js service worker (re)started at ${new Date(swStartedAt).toISOString()}`
);

function maybeBrowser() {
  if (typeof browser !== "undefined") {
    return browser;
  }
  return null;
}

function isHttpPage(url) {
  return typeof url === "string" && /^https?:\/\//i.test(url);
}

function logDiagnostics(label, response) {
  if (!response || typeof response !== "object") {
    console.log(`[virtue] ${label}: no diagnostics in response`, response);
    return;
  }
  const memMb = response.diag_mem_mb;
  const uptimeS = response.diag_uptime_s;
  const requestCount = response.diag_request_count;
  const daemonRunning = response.diag_daemon_running;
  const screenshotCount = response.diag_screenshot_count;
  const nsfwRunCount = response.diag_nsfw_run_count;
  const batchUploadCount = response.diag_batch_upload_count;
  console.log(
    `[virtue] ${label}: ext_mem=${memMb}MB ext_uptime=${
      typeof uptimeS === "number" ? uptimeS.toFixed(1) : uptimeS
    }s ext_requests=${requestCount} daemon_running=${daemonRunning} screenshots=${screenshotCount} nsfw_runs=${nsfwRunCount} batch_uploads=${batchUploadCount} sw_uptime=${(
      (Date.now() - swStartedAt) /
      1000
    ).toFixed(1)}s`
  );
}

async function sendNative(payload) {
  const b = maybeBrowser();
  const seq = ++messageSeq;
  const startedAt = Date.now();
  const label = `sendNative#${seq} type=${payload && payload.type} source=${
    payload && payload.source
  }`;
  console.log(`[virtue] -> ${label}`);

  if (!b || !b.runtime || typeof b.runtime.sendNativeMessage !== "function") {
    console.warn(`[virtue] ${label}: sendNativeMessage unavailable`);
    return;
  }

  try {
    const response = await b.runtime.sendNativeMessage(payload);
    console.log(`[virtue] <- ${label} ok in ${Date.now() - startedAt}ms`, response);
    logDiagnostics(label, response);
    return response;
  } catch (firstError) {
    // Safari/WebExtension runtime signatures vary; try a host argument fallback.
    console.warn(
      `[virtue] <- ${label} first attempt failed after ${Date.now() - startedAt}ms: ${
        firstError && firstError.message
      }; retrying with host arg`
    );
  }

  const hostCandidate = b.runtime.id || "native";
  try {
    const response = await b.runtime.sendNativeMessage(hostCandidate, payload);
    console.log(`[virtue] <- ${label} ok (fallback) in ${Date.now() - startedAt}ms`, response);
    logDiagnostics(label, response);
    return response;
  } catch (secondError) {
    console.error(
      `[virtue] <- ${label} FAILED after ${Date.now() - startedAt}ms: ${
        secondError && secondError.message
      }`
    );
    throw secondError;
  }
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

// MV3 service workers can be suspended by the OS at any time; onSuspend (if
// Safari's WebExtension runtime supports it) is the last line we'll see
// before that happens — a useful bookend to whatever heartbeat line came
// right before it.
if (b && b.runtime && typeof b.runtime.onSuspend?.addListener === "function") {
  b.runtime.onSuspend.addListener(() => {
    console.warn(
      `[virtue] background.js onSuspend fired at sw_uptime=${(
        (Date.now() - swStartedAt) /
        1000
      ).toFixed(1)}s — service worker is being torn down`
    );
  });
}

console.log(
  `[virtue] listeners registered: onInstalled=${Boolean(
    b?.runtime?.onInstalled?.addListener
  )} onStartup=${Boolean(b?.runtime?.onStartup?.addListener)} onMessage=${Boolean(
    b?.runtime?.onMessage?.addListener
  )} onActivated=${Boolean(b?.tabs?.onActivated?.addListener)} onUpdated=${Boolean(
    b?.tabs?.onUpdated?.addListener
  )} onSuspend=${Boolean(b?.runtime?.onSuspend?.addListener)}`
);

// Self-heartbeat: keeps producing a diagnostics line on a fixed cadence even
// when Safari has no visible/foreground tab, so a gap in these lines (rather
// than just "no more logs after some tab event") is a clean signal that
// either this background context or the native App Extension process died.
setInterval(() => {
  sendNative({ type: "ping", source: "background_heartbeat" }).catch(() => {});
}, HEARTBEAT_INTERVAL_MS);
