import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import { alias } from "./aliases";
import { bootstrapReport } from "./bootstrap-report";
import { paraglideVitePlugin } from "@inlang/paraglide-js";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  appType: "mpa",
  resolve: { alias },
  plugins: [
    bootstrapReport(),
    paraglideVitePlugin({
      project: "./project.inlang",
      outdir: "./src/shared/i18n",
      emitTsDeclarations: true,
      strategy: ["baseLocale"],
    }),
    svelte(),
    tailwindcss(),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: "es2022",
    rolldownOptions: {
      output: {
        // Per-message chunks can overflow the native custom-protocol request
        // budget during startup. Keep translation data in one shared asset;
        // heavy feature implementations retain their dynamic import boundaries.
        codeSplitting: {
          groups: [
            {
              name: "svelte-runtime",
              test: /node_modules[\\/]svelte[\\/]/u,
              tags: ["$initial"],
              priority: 3,
            },
            { name: "messages", test: /[\\/]src[\\/]shared[\\/]i18n[\\/]/u },
            { name: "ui-core", test: /[\\/]src[\\/]shared[\\/]ui[\\/]/u, tags: ["$initial"] },
            {
              name: "chrome-shared",
              test: /[\\/]src[\\/]shared[\\/](ipc|lib)[\\/]/u,
              tags: ["$initial"],
            },
          ],
        },
      },
      input: {
        browser: fileURLToPath(new URL("./browser.html", import.meta.url)),
        panel: fileURLToPath(new URL("./panel.html", import.meta.url)),
      },
    },
  },
});
