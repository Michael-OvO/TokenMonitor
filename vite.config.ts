import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig, type Plugin } from "vite";
import { resolve } from "path";

const host = process.env.TAURI_DEV_HOST;

// Quitting the app from the tray ends `tauri dev`, which kills only its own
// child (the npm wrapper); on Windows the Vite grandchild then kept port 1420.
// A debug build posts here on quit (`notify_dev_server_quit` in lib.rs).
const quitWithApp: Plugin = {
  name: "quit-with-app",
  configureServer(server) {
    server.middlewares.use("/__tm_quit", (req, res) => {
      if (req.method !== "POST") {
        res.statusCode = 405;
        res.end();
        return;
      }
      res.end("bye");
      server.close().finally(() => process.exit(0));
    });
  },
};

export default defineConfig(async () => ({
  plugins: [svelte(), quitWithApp],
  clearScreen: false,
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        "float-ball": resolve(__dirname, "float-ball.html"),
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
