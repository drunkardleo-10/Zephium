import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { alias } from "../../aliases";

/** Page frames and pictures come from the exported works, not the native media route. */
const media: Plugin = {
  name: "work-memory-media",
  enforce: "pre",
  load(id) {
    if (!id.endsWith("/src/domain/resources/media.ts")) return null;
    return `
      const lookup = () => globalThis.__harnessMedia;
      export function pageFrameUrl(attempt, step) { return lookup().frames.get(attempt + "-" + step) ?? null; }
      export function mediaUrl(_profile, digest) { return lookup().pictures.get(digest) ?? null; }
      export function mediaSize(bytes) { return bytes + " B"; }
    `;
  },
};

/** WORK_MEMORY_STUB=Diagram,Sheet renders those components as nothing, to find what holds memory. */
const stubbed = (process.env.WORK_MEMORY_STUB ?? "").split(",").filter(Boolean);
const stub: Plugin = {
  name: "work-memory-stub",
  enforce: "pre",
  load(id) {
    const name = id
      .split("/")
      .pop()
      ?.replace(/\.svelte$/u, "");
    return id.endsWith(".svelte") && name && stubbed.includes(name) ? "<div></div>" : null;
  },
};

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  publicDir: fileURLToPath(new URL("../../node_modules/.work-look", import.meta.url)),
  resolve: { alias },
  plugins: [media, stub, svelte(), tailwindcss()],
  logLevel: "warn",
  build: {
    target: "es2022",
    outDir: fileURLToPath(new URL("../../node_modules/.work-memory/dist", import.meta.url)),
    emptyOutDir: true,
    copyPublicDir: true,
  },
  preview: { port: 4719, strictPort: true },
});
