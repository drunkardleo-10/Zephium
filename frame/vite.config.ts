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
    // Bundled assets have no network latency. Eagerly preloading the whole
    // module graph can overflow the native protocol's 32-request admission
    // limit before the main stylesheet loads. Native module imports schedule
    // their dependencies; Vite still loads CSS before each lazy destination.
    modulePreload: false,
    rolldownOptions: {
      input: {
        browser: fileURLToPath(new URL("./browser.html", import.meta.url)),
        panel: fileURLToPath(new URL("./panel.html", import.meta.url)),
      },
    },
  },
});
