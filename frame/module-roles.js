import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const sourceRoot = fileURLToPath(new URL("./src", import.meta.url));
const relative = (filename) => path.relative(sourceRoot, filename).split(path.sep).join("/");
function resolveImport(filename, value) {
  if (typeof value !== "string") return null;
  const base = value.startsWith("$")
    ? path.join(sourceRoot, value.slice(1))
    : value.startsWith(".")
      ? path.resolve(path.dirname(filename), value)
      : null;
  if (!base) return null;
  return (
    [base, `${base}.ts`, `${base}.svelte`, path.join(base, "index.ts")].find(
      (candidate) => existsSync(candidate) && path.extname(candidate),
    ) ?? base
  );
}

/** Roles apply inside every feature, including future named capability submodules. */
export default {
  rules: {
    structure: {
      meta: {
        type: "problem",
        schema: [],
        messages: {
          placement:
            "Feature code belongs in components/, lib/, or tests/; only index.ts is a root API.",
          component: "Rendered components belong in components/, not lib/.",
          test: "Tests belong in the owning module's tests/ directory.",
          dependency: "Feature logic cannot depend on components or its own public barrel.",
          barrel:
            "Inside a feature, import the owning source module rather than its public barrel.",
        },
      },
      create(context) {
        const filename = context.filename;
        const parts = relative(filename).split("/");
        if (parts[0] !== "features") return {};
        const segments = parts.slice(2, -1);
        const role = segments.find((segment) => ["components", "lib", "tests"].includes(segment));
        const test = /\.test\.[jt]s$/u.test(filename);
        function check(node, source) {
          if (test) return;
          const target = resolveImport(filename, source?.value);
          if (!target) return;
          const destination = relative(target).split("/");
          if (destination[0] !== "features") return;
          const ownBarrel = destination[1] === parts[1] && destination.at(-1) === "index.ts";
          if (role === "lib" && (destination.includes("components") || ownBarrel)) {
            context.report({ node, messageId: "dependency" });
          } else if (ownBarrel && parts.at(-1) !== "index.ts") {
            context.report({ node, messageId: "barrel" });
          }
        }
        return {
          Program(node) {
            if (test && role !== "tests") context.report({ node, messageId: "test" });
            else if (!role && parts.at(-1) !== "index.ts")
              context.report({ node, messageId: "placement" });
            else if (role === "lib" && filename.endsWith(".svelte"))
              context.report({ node, messageId: "component" });
          },
          ImportDeclaration(node) {
            check(node, node.source);
          },
          ExportNamedDeclaration(node) {
            if (node.source) check(node, node.source);
          },
          ExportAllDeclaration(node) {
            check(node, node.source);
          },
          ImportExpression(node) {
            check(node, node.source);
          },
        };
      },
    },
  },
};
