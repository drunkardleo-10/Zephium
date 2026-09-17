import budgets from "./bundle-budgets.json";
import { checkBundleBudget } from "./bundle-budget";
import type { Plugin } from "vite";

/** Inspect emitted graphs: source folders alone cannot guarantee startup isolation. */
export function bootstrapReport(): Plugin {
  const surfaceStyles = new Map<string, Set<string>>();
  return {
    name: "zephium-bootstrap-boundaries",
    apply: "build",
    generateBundle(_options, bundle) {
      for (const item of Object.values(bundle)) {
        if (item.type !== "chunk") continue;
        for (const id of Object.keys(item.modules)) {
          if (
            /\/src\/(shared\/testing|gallery)\//u.test(id) ||
            /\/frame\/dev\//u.test(id) ||
            (/\/(tests|fixtures)\//u.test(id) && id.includes("/src/")) ||
            /\.test\.[jt]s$/u.test(id)
          )
            this.error(`Development-only module in production: ${id}`);
        }
      }
      const reports: Record<
        string,
        {
          entry: string;
          staticJsBytes: number;
          staticCssBytes: number;
          css: string[];
          modules: string[];
        }
      > = {};
      const roots: Array<{ name: string; file: string; surface: boolean }> = [];
      for (const name of ["browser", "panel"] as const) {
        const root = Object.values(bundle).find(
          (item) =>
            item.type === "chunk" && item.isEntry && item.facadeModuleId?.endsWith(`/${name}.html`),
        );
        if (!root || root.type !== "chunk") this.error(`Missing ${name} entry chunk`);
        roots.push({ name, file: root.fileName, surface: true });
      }
      for (const item of Object.values(bundle)) {
        if (item.type !== "chunk" || !item.isDynamicEntry || !item.facadeModuleId) continue;
        const at = item.facadeModuleId.lastIndexOf("/src/");
        if (at < 0) continue;
        const dependency = item.facadeModuleId.lastIndexOf("/node_modules/");
        roots.push({
          name:
            dependency >= 0
              ? `lazy:dependency/${item.facadeModuleId.slice(dependency + 14)}`
              : `lazy:${item.facadeModuleId.slice(at + 5)}`,
          file: item.fileName,
          surface: false,
        });
      }
      for (const root of roots) {
        const name = root.name;
        const visited = new Set<string>();
        const modules = new Set<string>();
        const css = new Set<string>();
        const walk = (file: string) => {
          if (visited.has(file)) return;
          visited.add(file);
          const item = bundle[file];
          if (!item || item.type !== "chunk") return;
          for (const id of Object.keys(item.modules)) {
            if (
              root.surface &&
              (/\/(?:features|domain)\/work\//u.test(id) || /\/(?:@xyflow|@tiptap)\//u.test(id))
            )
              this.error(`Work code in ${name} startup: ${id}`);
            const at = id.indexOf("/src/");
            if (at >= 0) modules.add(id.slice(at + 1));
          }
          for (const child of item.imports) walk(child);
          for (const style of (
            item as typeof item & { viteMetadata?: { importedCss: Set<string> } }
          ).viteMetadata?.importedCss ?? [])
            css.add(style);
        };
        walk(root.file);
        if (root.surface) surfaceStyles.set(name, css);
        const forbidden = [...modules].filter(
          (id) =>
            /^src\/features\/(sidebar|tabs|essentials|address|spaces|extensions|blocker|settings)\//u.test(
              id,
            ) ||
            /^src\/features\/tools\/components\/(previews\/|ToolSlot\.svelte|ToolFrame\.svelte)/u.test(
              id,
            ) ||
            /^src\/domain\/(tabs|blocker|extensions|runtime|permissions)\//u.test(id) ||
            [
              "src/app/browser/BrowserApp.svelte",
              "src/styles/global.css",
              "src/styles/browser.css",
              "src/session/tool-drafts.svelte.ts",
            ].includes(id),
        );
        if (name === "panel" && forbidden.length)
          this.error(`Panel eagerly loads browser/tool code: ${forbidden.join(", ")}`);
        reports[name] = {
          entry: root.file,
          staticJsBytes: [...visited].reduce((sum, file) => {
            const item = bundle[file];
            return sum + (item?.type === "chunk" ? Buffer.byteLength(item.code) : 0);
          }, 0),
          staticCssBytes: [...css].reduce((sum, file) => {
            const item = bundle[file];
            return sum + (item?.type === "asset" ? Buffer.byteLength(item.source) : 0);
          }, 0),
          css: [...css],
          modules: [...modules].sort(),
        };
      }
      const failures: string[] = [];
      for (const [name, report] of Object.entries(reports)) {
        const failure = checkBundleBudget(name, report, budgets);
        if (failure)
          failures.push(
            `${failure} (measured JS ${report.staticJsBytes}, CSS ${report.staticCssBytes})`,
          );
      }
      if (failures.length) this.error(failures.join("\n"));
      this.emitFile({
        type: "asset",
        fileName: "bootstrap-report.json",
        source: JSON.stringify(reports, null, 2),
      });
    },
    writeBundle(_options, bundle) {
      for (const [name, styles] of surfaceStyles) {
        const html = bundle[`${name}.html`];
        if (!html || html.type !== "asset") this.error(`Missing ${name} HTML`);
        for (const style of styles) {
          if (!String(html.source).includes(style))
            this.error(`${name} stylesheet missing from its HTML preload graph: ${style}`);
        }
      }
    },
  };
}
