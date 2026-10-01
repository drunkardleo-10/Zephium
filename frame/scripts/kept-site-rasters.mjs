// Regenerates the fixed favicon rasters native seeds when onboarding keeps a
// site, from the same marks the catalog draws. Native never decodes an image
// format for these: each file is the exact 32x32 RGBA the favicon store
// accepts. Run with `node scripts/kept-site-rasters.mjs` after changing a mark.
import { readFile, writeFile } from "node:fs/promises";
import { webkit } from "playwright";

const marks = new URL("../src/features/onboarding/marks/", import.meta.url);
const out = new URL("../../assets/kept-sites/", import.meta.url);
const catalog = await readFile(
  new URL("../src/features/onboarding/lib/catalog.ts", import.meta.url),
  "utf8",
);
const ids = [...catalog.matchAll(/^ {2}\{ id: "([a-z-]+)"/gmu)].map((match) => match[1]);
if (ids.length === 0) throw new Error("no catalog ids found");

const browser = await webkit.launch();
const page = await browser.newPage();
for (const id of ids) {
  const svg = await readFile(new URL(`${id}.svg`, marks));
  const rgba = await page.evaluate(async (source) => {
    const image = new Image();
    image.src = `data:image/svg+xml;base64,${source}`;
    await image.decode();
    const canvas = document.createElement("canvas");
    canvas.width = 32;
    canvas.height = 32;
    const context = canvas.getContext("2d");
    const scale = Math.min(32 / image.naturalWidth, 32 / image.naturalHeight);
    const width = image.naturalWidth * scale;
    const height = image.naturalHeight * scale;
    context.drawImage(image, (32 - width) / 2, (32 - height) / 2, width, height);
    return Array.from(context.getImageData(0, 0, 32, 32).data);
  }, svg.toString("base64"));
  await writeFile(new URL(`${id}.rgba`, out), Uint8Array.from(rgba));
}
await browser.close();
process.stdout.write(`wrote ${ids.length} rasters\n`);
