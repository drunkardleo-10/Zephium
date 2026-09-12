import { defineConfig } from "vitest/config";
import { playwright } from "@vitest/browser-playwright";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { alias } from "./aliases";

export default defineConfig({
  test: {
    projects: [
      {
        resolve: { alias },
        plugins: [svelte()],
        test: {
          name: "unit",
          environment: "node",
          include: ["src/**/*.test.ts", "*.test.ts"],
          exclude: ["**/*.component.test.ts"],
          restoreMocks: true,
          unstubGlobals: true,
        },
      },
      {
        resolve: { alias },
        plugins: [svelte()],
        test: {
          name: "component",
          include: ["src/**/*.component.test.ts"],
          browser: {
            enabled: true,
            headless: true,
            screenshotFailures: false,
            provider: playwright(),
            instances: [{ browser: process.platform === "darwin" ? "webkit" : "chromium" }],
          },
        },
      },
    ],
  },
});
