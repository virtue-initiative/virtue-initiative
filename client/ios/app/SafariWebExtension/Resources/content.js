(() => {
  if (typeof browser === "undefined" || !browser.runtime) {
    console.warn("[virtue] content.js: browser.runtime unavailable, extension inert on this page");
    return;
  }

  const TICK_INTERVAL_MS = 1200;
  let timer = null;

  console.log(`[virtue] content.js loaded at ${new Date().toISOString()} url=${location.href}`);

  function sendTick(source) {
    browser.runtime
      .sendMessage({ type: "virtue_capture_tick", source })
      .catch((error) => {
        // Most commonly happens right after the background service worker gets
        // evicted/restarted and hasn't re-registered its onMessage listener yet.
        console.warn(`[virtue] content.js sendTick(${source}) failed: ${error && error.message}`);
      });
  }

  function tickIfVisible(source) {
    if (document.hidden) {
      return;
    }
    sendTick(source);
  }

  function startTickLoop() {
    if (timer !== null) {
      return;
    }
    timer = window.setInterval(() => tickIfVisible("interval"), TICK_INTERVAL_MS);
  }

  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      sendTick("visibility_change");
    }
  });

  window.addEventListener(
    "focus",
    () => {
      sendTick("window_focus");
    },
    true
  );

  startTickLoop();
  sendTick("initial_load");
})();
