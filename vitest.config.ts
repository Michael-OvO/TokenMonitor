import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "build/**/*.test.mjs"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      include: ["src/**/*.ts", "build/**/*.mjs"],
      exclude: ["src/**/*.test.ts", "build/**/*.test.mjs", "src/vite-env.d.ts"],
    },
  },
});
