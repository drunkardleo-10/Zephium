import { readFileSync } from "node:fs";
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
            "@tiptap/extension-strike",
            "@tiptap/extension-horizontal-rule",
            "@tiptap/pm/history",
            "@tiptap/pm/model",
            "@tiptap/pm/state",
            "@tiptap/pm/view",
            "marked",
          ],
        },
        test: {
          name: "component",
          provide: {
            packagedStylePolicy: {
              styleSource: JSON.parse(
                readFileSync(new URL("../desktop/tauri.conf.json", import.meta.url), "utf8"),
              ).app.security.csp["style-src"] as string,
              // Match the HTML style elements to which Tauri adds nonce sources.
              inlineStyleCount: ["./browser.html", "./panel.html"].reduce(
                (count, path) =>
                  count +
                  (readFileSync(new URL(path, import.meta.url), "utf8").match(/<style(?:\s|>)/giu)
                    ?.length ?? 0),
                0,
              ),
            },
          },
          include: ["src/**/*.component.test.ts"],
          browser: {
            enabled: true,
            headless: true,
            screenshotFailures: false,
            // Retina is the display this chrome is drawn for, and the one
            // where a radius or a hairline can actually be judged.
            provider: playwright({ contextOptions: { deviceScaleFactor: 2 } }),
            instances: [{ browser: process.platform === "darwin" ? "webkit" : "chromium" }],
          },
        },
      },
    ],
  },
});
