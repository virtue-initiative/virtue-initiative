(() => {
  if (typeof browser === "undefined" || !browser.runtime) {
    return;
  }

  const TICK_INTERVAL_MS = 1200;
  let timer = null;

  function sendTick(source) {
    browser.runtime
      .sendMessage({ type: "virtue_capture_tick", source })
      .catch(() => {});
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
