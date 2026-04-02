import FloatBall from "./lib/components/FloatBall.svelte";
import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { isWindows } from "./lib/utils/platform";

// Disable right-click context menu
document.addEventListener("contextmenu", (e) => e.preventDefault());

// Set WebView background to fully transparent so no rectangle shows behind the ball.
// CSS `background: transparent` only affects the web layer; the WebView control itself
// needs an explicit transparent background color to avoid a visible box on Windows.
if (isWindows()) {
  getCurrentWebviewWindow()
    .setBackgroundColor({ red: 0, green: 0, blue: 0, alpha: 0 })
    .catch((e) => {
      console.debug("[float-ball] setBackgroundColor failed:", e);
    });
}

const app = mount(FloatBall, { target: document.getElementById("float-ball")! });

export default app;
