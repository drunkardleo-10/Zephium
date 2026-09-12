import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import budgets from "./work-bundle-budgets.json";
import { checkBundleBudget } from "./bundle-budget";
import { alias } from "./aliases";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

/** Separate artifact: the native desktop distribution never includes scenario data. */
export default defineConfig({
  resolve: { alias },
  plugins: [
    svelte(),
    tailwindcss(),
    {
      name: "work-preview-isolation",
      generateBundle(_options, bundle) {
        const report: Record<string, { js: number; css: number }> = {};
        for (const root of Object.values(bundle)) {
          if (
            root.type !== "chunk" ||
            !(root.isEntry || root.isDynamicEntry) ||
            !root.facadeModuleId
          )
            continue;
          const key = root.facadeModuleId.split("/").at(-1)!;
          const seen = new Set<string>();
          const styles = new Set<string>();
          const visit = (file: string) => {
            if (seen.has(file)) return;
            seen.add(file);
            const part = bundle[file];
            if (part?.type !== "chunk") return;
            part.imports.forEach(visit);
            for (const css of (
              part as typeof part & { viteMetadata?: { importedCss: Set<string> } }
            ).viteMetadata?.importedCss ?? [])
              styles.add(css);
          };
          visit(root.fileName);
          report[key] = {
            js: [...seen].reduce(
              (n, file) =>
                n + (bundle[file]?.type === "chunk" ? Buffer.byteLength(bundle[file].code) : 0),
              0,
            ),
            css: [...styles].reduce(
              (n, file) =>
                n + (bundle[file]?.type === "asset" ? Buffer.byteLength(bundle[file].source) : 0),
              0,
            ),
          };
        }
        for (const [name, limit] of Object.entries(budgets)) {
          const actual = report[name];
          if (!actual) this.error(`Missing Work entry: ${name}`);
          const failure = checkBundleBudget(
            name,
            { staticJsBytes: actual.js, staticCssBytes: actual.css },
            { [name]: limit },
          );
          if (failure) this.error(failure);
        }
        const totalJs = Object.values(bundle).reduce(
          (sum, output) => sum + (output.type === "chunk" ? Buffer.byteLength(output.code) : 0),
          0,
        );
        if (totalJs > 1_100_000)
          this.error(`Work preview distribution exceeds 1,100,000 JavaScript bytes: ${totalJs}`);
        this.emitFile({
          type: "asset",
          fileName: "work-bundle-report.json",
          source: JSON.stringify(report, null, 2),
        });
        for (const output of Object.values(bundle)) {
          if (output.type !== "chunk") continue;
          for (const id of Object.keys(output.modules)) {
            if (id.includes("/shared/ipc/") || id.includes("/@tauri-apps/"))
              this.error(`Native transport in presentation preview: ${id}`);
          }
        }
      },
    },
  ],
  server: { port: 1421, strictPort: true },
  build: {
    outDir: "dist-work-preview",
    target: "es2022",
    rolldownOptions: { input: fileURLToPath(new URL("./work-preview.html", import.meta.url)) },
  },
});
