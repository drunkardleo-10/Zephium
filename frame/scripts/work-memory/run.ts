/* eslint-disable no-console -- a measuring tool prints its readings. */
// Production-build memory run of the Work screen: `node scripts/work-memory/run.ts [--no-build]`.
// Builds scripts/work-memory, serves it, opens it in Playwright's WebKit, switches through the
// QA profile's real works (frame/node_modules/.work-look), and prints each phase's page counts
// and the footprint of every WebKit process the run started.
import { execFileSync, spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { webkit } from "playwright";

const frame = fileURLToPath(new URL("../..", import.meta.url));
const config = "scripts/work-memory/vite.config.ts";
const vite = (args) => [
  fileURLToPath(new URL("../../node_modules/vite/bin/vite.js", import.meta.url)),
  ...args,
];

if (!process.argv.includes("--no-build"))
  execFileSync(process.execPath, vite(["build", "--config", config]), {
    cwd: frame,
    stdio: "ignore",
  });

const webkitPids = () => {
  try {
    return execFileSync("pgrep", ["-f", "ms-playwright/webkit.*WebKit\\.(WebContent|GPU)"])
      .toString()
      .split("\n")
      .filter(Boolean);
  } catch {
    return [];
  }
};
const footprint = (pid) => {
  const out = execFileSync("footprint", [pid]).toString();
  const pick = (pattern) => out.match(pattern)?.[1]?.replace(/\s+/g, "") ?? "0";
  const name = out.match(/com\.apple\.WebKit\.(\w+)/)?.[1] ?? "?";
  const cpu = execFileSync("ps", ["-o", "time=", "-p", pid]).toString().trim();
  return `${pid} ${name} total=${pick(/phys_footprint:\s+([\d.]+ \w+)/)} peak=${pick(/phys_footprint_peak:\s+([\d.]+ \w+)/)} malloc=${pick(/\n\s*([\d.]+ \w+)\s+[\d.]+ \w+\s+[\d.]+ \w+\s+\d+\s+WebKit Malloc\n/)} graphics=${pick(/\n\s*([\d.]+ \w+)\s+[\d.]+ \w+\s+[\d.]+ \w+\s+\d+\s+Owned physical footprint \(unmapped\) \(graphics\)\n/)} cpu=${cpu}`;
};

const server = spawn(process.execPath, vite(["preview", "--config", config]), {
  cwd: frame,
  stdio: "ignore",
});
const before = new Set(webkitPids());
const browser = await webkit.launch({ headless: !process.argv.includes("--headed") });
try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
  });
  const page = await context.newPage();
  page.on("pageerror", (error) => console.log(`page error: ${error.message}\n${error.stack}`));
  for (let tries = 0; ; tries++) {
    try {
      const only = process.argv.find((arg) => arg.startsWith("--only="))?.slice(7);
      await page.goto(
        `http://localhost:4719/?${process.argv.includes("--same") ? "same&" : ""}${process.argv.includes("--build-only") ? "build-only&" : ""}${only ? `only=${only}` : ""}`,
      );
      break;
    } catch (error) {
      if (tries > 50) throw error;
      await new Promise((done) => setTimeout(done, 200));
    }
  }
  await page.waitForFunction(() => "harness" in window);
  const ours = () => webkitPids().filter((pid) => !before.has(pid));
  const phase = async (label) => {
    const counts = await page.evaluate(() => window.harness.inPage());
    console.log(`\n== ${label} ${JSON.stringify(counts)}`);
    for (const pid of ours()) console.log(footprint(pid));
  };
  const collect = () => page.evaluate(() => window.harness.collect());
  await phase("loaded");
  const works = await page.evaluate(() =>
    window.harness.start().catch((error) => `failed: ${error}\n${error.stack}`),
  );
  console.log(`works: ${works}`);
  await phase("empty");
  await collect();
  await phase("empty-collected");
  if (process.argv.includes("--each")) {
    for (const id of await page.evaluate(() => window.harness.ids())) {
      const ms = await page.evaluate((one) => window.harness.open(one), id);
      await phase(`open ${id.slice(-4)} ${ms}ms`);
      await collect();
      await phase(`open ${id.slice(-4)} collected`);
    }
  }
  for (let round = 1; round <= 3; round++) {
    await page.evaluate(() => window.harness.round());
    await phase(`round${round}`);
    await collect();
    await phase(`round${round}-collected`);
  }
  const freshSets = Number(process.argv.find((arg) => arg.startsWith("--fresh="))?.slice(8) ?? 2);
  for (let mark = 20; mark < 20 + freshSets; mark++) {
    await page.evaluate((m) => window.harness.fresh(m), mark);
    await collect();
    await phase(`fresh${mark}-collected`);
    if (process.argv.includes("--detached"))
      console.log(JSON.stringify(await page.evaluate(() => window.harness.detached())));
  }
  await page.waitForTimeout(30_000);
  await phase("rest-30s");
  await page.waitForTimeout(10_000);
  await phase("rest-40s");
  if (process.argv.includes("--unmount")) {
    await page.evaluate(() => window.harness.close());
    await collect();
    await phase("unmounted-collected");
  }
  if (process.argv.includes("--detail"))
    for (const pid of ours())
      console.log(execFileSync("footprint", [pid]).toString().split("\n").slice(0, 24).join("\n"));
  console.log(`unmocked: ${JSON.stringify(await page.evaluate(() => window.harness.unknown()))}`);
} finally {
  await browser.close();
  server.kill();
}
