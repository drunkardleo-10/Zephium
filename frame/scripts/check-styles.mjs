import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import postcss from "postcss";

const dist = new URL("../dist/", import.meta.url);
const files = (await readdir(dist, { recursive: true })).filter((path) => path.endsWith(".css"));
if (!files.length) throw new Error("No built CSS found; run pnpm -C frame build first");
const failures = [];
for (const file of files) {
  const root = postcss.parse(await readFile(new URL(file, dist), "utf8"), { from: file });
  root.walkDecls((declaration) => {
    const { prop, value } = declaration;
    // Blur is a design/performance decision, not a blanket CSS prohibition.
    if (prop !== "box-shadow" && !prop.startsWith("--shadow-")) return;
    // Named shadows for surfaces and thumbs that physically float are the
    // only intentional exception; ordinary controls keep a contact shadow.
    if (
      [
        "--shadow-popover",
        "--shadow-menu",
        "--shadow-overlay",
        "--shadow-sheet",
        "--shadow-float",
        "--shadow-thumb",
        "--shadow-thumb-grab",
      ].includes(prop)
    )
      return;
    // Remove color functions before examining geometry, so RGB numbers cannot
    // be mistaken for lengths. Variables are checked at their declarations.
    const geometry = value.replace(/(?:rgba?|hsla?|oklch|oklab|color-mix|var)\([^)]*\)/giu, "");
    for (const shadow of postcss.list.comma(geometry)) {
      const lengths = shadow.match(/(?<![\w.-])-?(?:\d*\.)?\d+(?:px|rem|em)?(?![\w.-])/gu) ?? [];
      const blur = lengths[2];
      if (!blur) continue;
      const magnitude = Number.parseFloat(blur) * (/r?em$/u.test(blur) ? 16 : 1);
      if (magnitude > 3)
        failures.push(`${file}: ${prop} exceeds 3px contact-shadow blur: ${value}`);
    }
  });
}

// Tailwind's size-named radius scale is removed from the theme, so a class
// such as `rounded-md` compiles to nothing and the element silently goes
// square. Only the role-named radii in tokens.css exist.
const source = new URL("../src/", import.meta.url);
const sizeRadius =
  /(?<![\w-])rounded(?:-(?:[trblse]|tl|tr|bl|br|ss|se|es|ee))?-(?:xs|sm|md|lg|xl|[2-4]xl)(?![\w-])/u;
for (const file of await readdir(source, { recursive: true })) {
  if (!/\.(?:svelte|ts)$/u.test(file) || /(?:^|\/)tests\//u.test(file)) continue;
  const lines = (await readFile(new URL(file, source), "utf8")).split("\n");
  lines.forEach((line, index) => {
    const match = line.match(sizeRadius);
    if (match) failures.push(`src/${file}:${index + 1}: size-named radius ${match[0]}`);
  });
}
if (failures.length) throw new Error(failures.join("\n"));
process.stdout.write(`Checked ${files.length} emitted CSS files in ${fileURLToPath(dist)}\n`);
