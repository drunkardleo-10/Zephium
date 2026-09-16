import { defineConfig } from "vitest/config";
import { playwright } from "@vitest/browser-playwright";
import tailwindcss from "@tailwindcss/vite";
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
        plugins: [svelte(), tailwindcss()],
        optimizeDeps: {
          include: [
            "layerchart/svg",
            "@xyflow/svelte",
            "@tiptap/core",
            "@tiptap/extension-document",
            "@tiptap/extension-paragraph",
            "@tiptap/extension-text",
            "@tiptap/extension-heading",
            "@tiptap/extension-bold",
            "@tiptap/extension-italic",
            "@tiptap/extension-code",
            "@tiptap/extension-bullet-list",
            "@tiptap/extension-ordered-list",
            "@tiptap/extension-list-item",
            "@tiptap/extension-blockquote",
            "@tiptap/extension-code-block",
            "@tiptap/extension-hard-break",
          ],
        },
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
