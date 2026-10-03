import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { deepStrictEqual } from "node:assert";
import { createRequire } from "node:module";
const frameRequire = createRequire(new URL("../frame/package.json", import.meta.url));
const eslintRequire = createRequire(frameRequire.resolve("eslint"));
const espreeRequire = createRequire(eslintRequire.resolve("espree"));
const { parse, tokenizer } = espreeRequire("acorn");

const assets = new URL("../crates/zephium-agentic/assets/", import.meta.url);
const source = readFileSync(new URL("semantic-runtime-v1.js", assets), "utf8");
let end = 0;
let compact = "";
for (const token of tokenizer(source, { ecmaVersion: "latest" })) {
  const gap = source.slice(end, token.start);
  compact += /\r|\n/.test(gap) ? "\n" : gap.length ? " " : "";
  compact += source.slice(token.start, token.end);
  end = token.end;
}
compact += "\n";
const ast = (text) => JSON.parse(JSON.stringify(
  parse(text, { ecmaVersion: "latest" }),
  (key, value) => ["start", "end"].includes(key) ? undefined : value,
));
deepStrictEqual(ast(compact), ast(source));
const hash = (text) => createHash("sha256").update(text).digest("hex");
const manifest = `${JSON.stringify({ source: hash(source), compact: hash(compact) })}\n`;
for (const [name, text] of [
  ["semantic-runtime-cdp-v1.js", compact],
  ["semantic-runtime-cdp-v1.json", manifest],
]) {
  const path = new URL(name, assets);
  if (process.argv.slice(2).length === 0) writeFileSync(path, text);
  else if (process.argv.length === 3 && process.argv[2] === "--check") {
    deepStrictEqual(readFileSync(path, "utf8"), text);
  } else throw new Error("Expected no arguments or --check");
}
