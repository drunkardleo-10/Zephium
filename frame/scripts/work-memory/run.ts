/* eslint-disable no-console -- a measuring tool prints its readings. */
// Production-build memory run of the Work screen: `node scripts/work-memory/run.ts [--no-build]`.
// Builds scripts/work-memory, serves it, opens it in Playwright's WebKit, switches through the
// QA profile's real works (frame/node_modules/.work-look), and prints each phase's page counts
// and the footprint of every WebKit process the run started.
import { execFileSync, spawn } from "node:child_process";
import { existsSync } from "node:fs";
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

const hostMode = process.argv.includes("--host");
const webkitPids = () => {
  try {
    return execFileSync("pgrep", [
      "-f",
      hostMode
        ? "com\\.apple\\.WebKit\\.(WebContent|GPU)"
        : "ms-playwright/webkit.*WebKit\\.(WebContent|GPU)",
    ])
      .toString()
      .split("\n")
      .filter(Boolean);
  } catch {
    return [];
  }
};
const footprint = (pid) => {
  const out = execFileSync("footprint", [pid]).toString();
  const row = (name) =>
    out
      .match(
        new RegExp(
          `\\n\\s*([\\d.]+ \\w+)\\s+[\\d.]+ \\w+\\s+[\\d.]+ \\w+\\s+\\d+\\s+${name.replace(/[()]/g, "\\$&")}\\n`,
        ),
      )?.[1]
      ?.replace(/\s+/g, "") ?? "0";
  const pick = (pattern) => out.match(pattern)?.[1]?.replace(/\s+/g, "") ?? "0";
  const name = out.match(/com\.apple\.WebKit\.(\w+)/)?.[1] ?? "?";
  const cpu = execFileSync("ps", ["-o", "time=", "-p", pid]).toString().trim();
  return `${pid} ${name} total=${pick(/phys_footprint:\s+([\d.]+ \w+)/)} peak=${pick(/phys_footprint_peak:\s+([\d.]+ \w+)/)} malloc=${row("WebKit Malloc")} gfx=${row("Untagged (graphics)")} owned=${row("Owned physical footprint (unmapped) (graphics)")} untagged=${row("Untagged")} jit=${row("JS JIT Generated Code")} cpu=${cpu}`;
};

const CENSUS =
  /^\s*(\d+)\s+(\d+)\s+[\d.]+\s+(WebCore::(Comment|Text|HTMLDivElement|HTMLSpanElement|HTMLElement|SVGPathElement|SVGSVGElement|WebAnimation|KeyframeEffect|CanvasRenderingContext2D|HTMLImageElement|HTMLCanvasElement|RenderBlockFlow|RenderInline|RenderImage|RenderLayer|StyleRule|ImmutableStyleProperties|MutableStyleProperties|CachedImage|BitmapImage|Document|Frame\w*)|WebKit::RemoteImageBufferProxy|JSC::\w*Code\w*|non-object)\s/;
function census(pid) {
  const out = execFileSync("heap", ["--sortBySize", pid], { maxBuffer: 1 << 28 }).toString();
  const total = out.match(/All zones: (\d+) nodes \((\d+) bytes\)/);
  const rows = out
    .split("\n")
    .map((line) => line.match(CENSUS))
    .filter(Boolean)
    .map((row) => `${row[3]}=${row[1]}/${(Number(row[2]) / 1048576).toFixed(1)}MB`);
  return `  heap ${total?.[1]} nodes ${(Number(total?.[2]) / 1048576).toFixed(0)}MB: ${rows.join(" ")}`;
}

const server = spawn(process.execPath, vite(["preview", "--config", config]), {
  cwd: frame,
  stdio: "ignore",
});
const before = new Set(webkitPids());

/** The system WebKit in a WKWebView host (scripts/work-memory/host), driven line by line over stdin. */
function nativeHost() {
  const binary = fileURLToPath(new URL("../../node_modules/.work-memory/host", import.meta.url));
  const source = fileURLToPath(new URL("host/main.swift", import.meta.url));
  execFileSync("swiftc", ["-O", source, "-o", binary], { stdio: "ignore" });
  const child = spawn(binary, [], { stdio: ["pipe", "pipe", "inherit"] });
  const waiting = [];
  let buffer = "";
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    for (let cut = buffer.indexOf("\n"); cut >= 0; cut = buffer.indexOf("\n")) {
      const line = buffer.slice(0, cut);
      buffer = buffer.slice(cut + 1);
      waiting.shift()?.(JSON.parse(line));
    }
  });
  const ready = new Promise((done) => waiting.push(done));
  let queue = ready;
  const send = (command) => {
    const reply = queue.then(
      () =>
        new Promise((done) => {
          waiting.push(done);
          child.stdin.write(`${command}\n`);
        }),
    );
    queue = reply.catch(() => {});
    return reply;
  };
  const script = (fn, arg) => `return await (${fn})(${JSON.stringify(arg ?? null)});`;
  const page = {
    goto: (url) => send(`load ${url}`),
    async evaluate(fn, arg) {
      const reply = await send(`eval ${script(fn, arg).replaceAll("\n", " ")}`);
      if ("error" in reply) throw new Error(reply.error);
      return reply.ok;
    },
    async waitForFunction(fn) {
      while (!(await page.evaluate(fn))) await new Promise((done) => setTimeout(done, 200));
    },
    waitForTimeout: (ms) => new Promise((done) => setTimeout(done, ms)),
    on() {},
    memoryCache: () => send("memory-cache"),
    call: (selector) => send(`call ${selector}`),
    pid: async () => String((await send("pid")).pid),
  };
  const browser = { close: () => child.kill() };
  return { page, browser };
}

const playwrightPage = async () => {
  const browser = await webkit.launch({ headless: !process.argv.includes("--headed") });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
  });
  const page = await context.newPage();
  page.on("pageerror", (error) => console.log(`page error: ${error.message}\n${error.stack}`));
  return { page, browser };
};
const { page, browser } = hostMode ? nativeHost() : await playwrightPage();
try {
  for (let tries = 0; ; tries++) {
    try {
      const only = process.argv.find((arg) => arg.startsWith("--only="))?.slice(7);
      await page.goto(
        `http://localhost:${process.env.WORK_MEMORY_PORT ?? 4719}/?${process.argv
          .filter((arg) => arg.startsWith("--q="))
          .map((arg) => `${arg.slice(4)}&`)
          .join(
            "",
          )}${process.argv.includes("--same") ? "same&" : ""}${process.argv.includes("--build-only") ? "build-only&" : ""}${only ? `only=${only}` : ""}`,
      );
      break;
    } catch (error) {
      if (tries > 50) throw error;
      await new Promise((done) => setTimeout(done, 200));
    }
  }
  await page.waitForFunction(() => "harness" in window);
  const hostPid = hostMode ? Number(await page.pid()) : 0;
  const ours = () =>
    webkitPids().filter((pid) =>
      hostMode
        ? Number(pid) === hostPid || (Number(pid) < hostPid && Number(pid) > hostPid - 3)
        : !before.has(pid),
    );
  const phase = async (label) => {
    const counts = await page.evaluate(() => window.harness.inPage()).catch(() => ({ gone: true }));
    if (!process.argv.includes("--attached")) delete counts.attached;
    console.log(`\n== ${label} ${JSON.stringify(counts)}`);
    for (const pid of ours()) console.log(footprint(pid));
    if (process.argv.includes("--census") && hostMode) console.log(census(String(hostPid)));
  };
  let answered = 0;
  if (hostMode && process.argv.includes("--native"))
    setInterval(() => {
      void page
        .evaluate(() => window.harness.released())
        .then(async (asked) => {
          if (asked === answered) return;
          answered = asked;
          await page.memoryCache();
          console.log(`  native: memory cache emptied (#${asked})`);
        })
        .catch(() => {});
    }, 1000).unref();
  const collect = async () => {
    if (hostMode && process.argv.includes("--gc")) {
      await page.call("_garbageCollectJavaScriptObjectsForTesting");
      await page.waitForTimeout(1500);
    }
    await page.evaluate(() => window.harness.collect());
    if (hostMode && process.argv.includes("--gc")) {
      await page.call("_garbageCollectJavaScriptObjectsForTesting");
      await page.waitForTimeout(3000);
    }
  };
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
  const repeat = process.argv.find((arg) => arg.startsWith("--repeat="))?.slice(9);
  if (repeat) {
    const [index, times] = repeat.split("x").map(Number);
    const target = (await page.evaluate(() => window.harness.ids()))[index];
    for (let time = 1; time <= times; time++) {
      await page.evaluate((one) => window.harness.open(one), target);
      await page.evaluate(() => window.harness.leave());
      if (time % 2 === 0 || time === times) {
        await collect();
        await phase(`repeat${time}`);
        if (process.argv.includes("--detached"))
          console.log(JSON.stringify(await page.evaluate(() => window.harness.detached())));
      }
    }
    if (process.argv.includes("--hold")) {
      console.log(`HOLD ${hostPid}`);
      while (!existsSync("/tmp/work-memory-release")) await page.waitForTimeout(1000);
    }
    await browser.close();
    server.kill();
    process.exit(0);
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
  for (const step of (process.argv.find((arg) => arg.startsWith("--do="))?.slice(5) ?? "")
    .split(",")
    .filter(Boolean)) {
    if (step === "memory-cache") await page.memoryCache();
    else if (step === "gc") await page.call("_garbageCollectJavaScriptObjectsForTesting");
    else await page.call(step);
    await page.waitForTimeout(5000);
    await phase(`after ${step}`);
  }
  if (process.argv.includes("--hold") && !process.argv.some((arg) => arg.startsWith("--repeat="))) {
    console.log(`HOLD ${hostPid}`);
    while (!existsSync("/tmp/work-memory-release")) await page.waitForTimeout(1000);
  }
  if (process.argv.includes("--reload")) {
    await page.goto("about:blank");
    await page.waitForTimeout(5000);
    await phase("blank");
  }
  if (process.argv.includes("--heap"))
    for (const pid of ours().filter((one) => footprint(one).includes(" WebContent "))) {
      const run = (command, args) => execFileSync(command, args, { maxBuffer: 1 << 28 }).toString();
      console.log(run("vmmap", ["--summary", pid]).split("\n").slice(0, 60).join("\n"));
      console.log(run("heap", ["--sortBySize", pid]).split("\n").slice(0, 60).join("\n"));
    }
  if (process.argv.includes("--detail"))
    for (const pid of ours())
      console.log(execFileSync("footprint", [pid]).toString().split("\n").slice(0, 24).join("\n"));
  console.log(`unmocked: ${JSON.stringify(await page.evaluate(() => window.harness.unknown()))}`);
} finally {
  await browser.close();
  server.kill();
}
