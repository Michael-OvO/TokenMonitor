import { invoke } from "@tauri-apps/api/core";
import FloatBall from "./lib/components/FloatBall.svelte";
import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { logger, type LogLevel } from "./lib/utils/logger.js";

logger.setIpcReady();
void invoke<LogLevel>("get_log_level")
  .then((level) => {
    logger.setLevel(level);
    logger.info("floatBall", `Float ball webview logger initialized: level=${level}`);
  })
  .catch((e) => {
    logger.warn(
      "floatBall",
      `Float ball webview failed to load log level: ${e instanceof Error ? e.message : String(e)}`,
    );
  });

// Disable right-click context menu
document.addEventListener("contextmenu", (e) => e.preventDefault());

// Set WebView background to fully transparent so no rectangle shows behind the ball.
// CSS `background: transparent` only affects the web layer; the WebView control itself
// needs an explicit transparent background color to avoid stale native buffers.
getCurrentWebviewWindow()
  .setBackgroundColor({ red: 0, green: 0, blue: 0, alpha: 0 })
  .catch((e) => {
    logger.warn(
      "floatBall",
      `setBackgroundColor failed: ${e instanceof Error ? e.message : String(e)}`,
    );
  });

logger.info("floatBall", "Float ball webview bootstrap");

function lockViewportScrolling() {
  // Guard against zero/implausible dimensions during WebKitGTK's initial layout pass.
  if (window.innerWidth < 10 || window.innerHeight < 10) {
    logger.debug(
      "floatBall",
      `Viewport lock skipped: implausible size ${window.innerWidth}x${window.innerHeight}`,
    );
    return;
  }
  const width = `${window.innerWidth}px`;
  const height = `${window.innerHeight}px`;
  const root = document.documentElement;
  const body = document.body;

  root.style.setProperty("--float-ball-viewport-w", width);
  root.style.setProperty("--float-ball-viewport-h", height);
  root.style.overflow = "hidden";
  root.style.overflowX = "hidden";
  root.style.overflowY = "hidden";
  root.style.width = width;
  root.style.height = height;
  root.style.maxWidth = width;
  root.style.maxHeight = height;

  body.style.overflow = "hidden";
  body.style.overflowX = "hidden";
  body.style.overflowY = "hidden";
  body.style.position = "fixed";
  body.style.inset = "0";
  body.style.width = width;
  body.style.height = height;
  body.style.maxWidth = width;
  body.style.maxHeight = height;

  const scrollingElement = document.scrollingElement;
  if (scrollingElement) {
    scrollingElement.scrollLeft = 0;
    scrollingElement.scrollTop = 0;
  }
  window.scrollTo(0, 0);

  logger.debug(
    "floatBall",
    `Viewport locked: inner=${window.innerWidth}x${window.innerHeight} client=${root.clientWidth}x${root.clientHeight} scroll=${root.scrollWidth}x${root.scrollHeight}`,
  );
}

// Defer first call by one RAF on Linux to let WebKitGTK finish initial layout.
requestAnimationFrame(() => lockViewportScrolling());
window.addEventListener("resize", lockViewportScrolling);
window.addEventListener("scroll", () => window.scrollTo(0, 0), { passive: true });

const app = mount(FloatBall, { target: document.getElementById("float-ball")! });

export default app;
