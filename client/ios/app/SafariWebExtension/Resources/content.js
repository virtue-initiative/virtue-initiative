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

  // virtueinitiative.org/check-extension shows whether this extension is on.
  // Only Virtue's own site gets an answer, so other sites can't use the page's
  // element to find out that someone runs Virtue.
  function isVirtueSite() {
    const host = location.hostname;
    return (
      host === "virtueinitiative.org" ||
      host.endsWith(".virtueinitiative.org") ||
      host === "localhost" ||
      host.endsWith(".localhost")
    );
  }

  function reportStatusToCheckPage() {
    const target = document.getElementById("virtue-extension-check");
    if (!target || !isVirtueSite()) {
      return;
    }
    browser.runtime
      .sendMessage({ type: "virtue_extension_status" })
      .then((status) => {
        status = status || {};
        target.dataset.extension = "on";
        target.dataset.allSites = String(status.all_sites);
        target.dataset.privateAllowed = String(status.private_allowed);
        target.dataset.paused = String(Boolean(status.paused));
      })
      .catch(() => {
        // This script is running, so the extension is on even without details.
        target.dataset.extension = "on";
      });
  }

  reportStatusToCheckPage();
})();
