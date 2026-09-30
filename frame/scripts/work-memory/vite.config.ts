import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { alias } from "../../aliases";

const HOST = `http://zephium-media.localhost:${process.env.WORK_MEMORY_PORT ?? 4719}`;

/** What the native media route does with `?w=`: a copy scaled down to that width, made once. */
const scaled: Plugin = {
  name: "work-memory-scaled",
  configurePreviewServer(server) {
    const cache = join(tmpdir(), "work-memory-scaled");
    mkdirSync(cache, { recursive: true });
    const dist = fileURLToPath(
      new URL(
        `../../node_modules/.work-memory/${process.env.WORK_MEMORY_DIST ?? "dist"}`,
        import.meta.url,
      ),
    );
    server.middlewares.use((request, response, next) => {
      const url = new URL(request.url ?? "/", "http://x");
      const width = Number(url.searchParams.get("w"));
      const source = join(dist, decodeURIComponent(url.pathname));
      if (
        !width ||
        process.env.WORK_MEMORY_FULL ||
        !/\.(png|jpe?g)$/u.test(source) ||
        !existsSync(source)
      )
        return next();
      const natural = Number(
        /pixelWidth: (\d+)/u.exec(
          execFileSync("sips", ["-g", "pixelWidth", source]).toString(),
        )?.[1],
      );
      const file =
        natural > width ? join(cache, `${width}-${source.replaceAll("/", "_")}`) : source;
      if (file !== source && !existsSync(file))
        execFileSync("sips", ["-Z", String(width), source, "--out", file], { stdio: "ignore" });
      response.setHeader("Content-Type", /\.png$/u.test(source) ? "image/png" : "image/jpeg");
      response.setHeader("Access-Control-Allow-Origin", "*");
      response.end(readFileSync(file));
    });
  },
};

/** Page frames and pictures come from the exported works, not the native media route. */
const media: Plugin = {
  name: "work-memory-media",
  enforce: "pre",
  load(id) {
    if (!id.endsWith("/src/domain/resources/media.ts")) return null;
    return `
      const lookup = () => globalThis.__harnessMedia;
      const on = (path) => path ? "${HOST}" + path : null;
      export function pageFrameUrl(attempt, step) { return on(lookup().frames.get(attempt + "-" + step)); }
      export function mediaUrl(_profile, digest, width = 720) {
        const url = on(lookup().pictures.get(digest));
        return url && url + (url.includes("?") ? "&" : "?") + "w=" + width;
      }
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

/** WORK_MEMORY_PATCH=noinspect tries a change to a component without editing it. */
const patch: Plugin = {
  name: "work-memory-patch",
  enforce: "pre",
  transform(code, id) {
    if (!process.env.WORK_MEMORY_PATCH || !id.endsWith("/run/PageFace.svelte")) return null;
    if (process.env.WORK_MEMORY_PATCH === "noinspect")
      return code.replace(
        "onload={(event) => inspect(event.currentTarget as HTMLImageElement)}",
        "",
      );
    if (process.env.WORK_MEMORY_PATCH === "canvas")
      return code
        .replace("const small = $derived(!!scale?.far && !live);", "const small = $derived(!live);")
        .replace("width: 224", "width: 360");
    if (process.env.WORK_MEMORY_PATCH === "canvasall")
      return code
        .replace("const small = $derived(!!scale?.far && !live);", "const small = $derived(true);")
        .replace("width: 224", "width: 360");
    if (process.env.WORK_MEMORY_PATCH === "noimg")
      return code.replace(/<img\s+src=\{frame\}[\s\S]*?\/>/u, "");
    if (process.env.WORK_MEMORY_PATCH === "noglyph")
      return code.replace(/<HostGlyph[\s\S]*?\/>/gu, "");
    if (process.env.WORK_MEMORY_PATCH === "noorb") return code.replace(/<Orb[^>]*\/>/gu, "");
    return null;
  },
};

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  publicDir: fileURLToPath(new URL("../../node_modules/.work-look", import.meta.url)),
  resolve: { alias },
  plugins: [media, stub, patch, scaled, svelte(), tailwindcss()],
  logLevel: "warn",
  build: {
    target: "es2022",
    outDir: fileURLToPath(
      new URL(
        `../../node_modules/.work-memory/${process.env.WORK_MEMORY_DIST ?? "dist"}`,
        import.meta.url,
      ),
    ),
    emptyOutDir: true,
    copyPublicDir: true,
  },
  preview: {
    allowedHosts: true,
    port: Number(process.env.WORK_MEMORY_PORT ?? 4719),
    strictPort: true,
  },
});
