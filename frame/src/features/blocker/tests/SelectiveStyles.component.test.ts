import { afterEach, expect, inject, test, vi } from "vitest";

declare module "vitest" {
  export interface ProvidedContext {
    contentStyleSource: string;
  }
}
type StyleApi = {
  inspect(): { token: string; url: string };
  inspectEncoded(): string;
  reuseSubscription(token: string, url: string, generation: string, fingerprint: string): boolean;
  subscription(
    token: string,
    url: string,
    generation: string,
    fingerprint: string,
    css: string,
    index: string | boolean,
    exceptions: string,
  ): boolean;
};
type TokenBatch = {
  token: string;
  url: string;
  subscription: string;
  serial: number;
  tokens: string[];
};
type PullApi = StyleApi & {
  pullGeneric(token: string, url: string, fingerprint: string): Promise<string | null> | null;
  applyGeneric(
    token: string,
    url: string,
    fingerprint: string,
    serial: number,
    selectors: string[],
  ): boolean;
};
let frame: HTMLIFrameElement | undefined;
afterEach(() => {
  vi.restoreAllMocks();
  frame?.remove();
  frame = undefined;
});

test("exhausted generic budget stops discovery until a replacement policy arrives", async () => {
  const { win, api, token, url, update, display } = await fixture();
  // WebKit has no requestIdleCallback; the script falls back to setTimeout.
  const idle =
    "requestIdleCallback" in win
      ? vi.spyOn(win, "requestIdleCallback")
      : vi.spyOn(win, "setTimeout");
  const index = JSON.stringify([
    [".ad", Array.from({ length: 2049 }, (_, i) => `.ad[data-slot="${i}"]`)],
  ]);
  expect(api.subscription(token, url, "0000000000000001", "c".repeat(64), "", index, "[]")).toBe(
    true,
  );
  const rules = () => [...win.document.adoptedStyleSheets].flatMap((s) => [...s.cssRules]).length;
  await expect.poll(rules).toBe(2048);
  const scheduled = idle.mock.calls.length;
  const later = win.document.createElement("div");
  later.className = "ad";
  later.id = "late";
  win.document.body.append(later);
  await new Promise<void>((resolve) => {
    win.requestAnimationFrame(() => win.requestAnimationFrame(() => resolve()));
  });
  expect(idle.mock.calls.length).toBe(scheduled);
  expect(rules()).toBe(2048);
  expect(update(2, true)).toBe(true);
  await expect.poll(() => display("#late")).toBe("none");
  await expect.poll(rules).toBe(2);
});

async function fixture() {
  frame = document.createElement("iframe");
  const loaded = new Promise<void>((resolve) => {
    frame!.onload = () => resolve();
  });
  frame.srcdoc = `<div class="ad">Ad</div><div class="except">Keep me</div><main>Useful content</main><script>${inject("contentStyleSource")}</script>`;
  document.body.append(frame);
  await loaded;
  const win = frame.contentWindow!;
  const api = (win as unknown as { __zephium_content_style_v1__: StyleApi })
    .__zephium_content_style_v1__;
  const { token, url } = api.inspect();
  const index = JSON.stringify([
    [".ad", [".ad"]],
    [".except", [".except"]],
    ["#late", ["#late"]],
    ...Array.from({ length: 1000 }, (_, i) => [`.unused-${i}`, [`.unused-${i}`]]),
  ]);
  const update = (generation: number, enabled: boolean) =>
    api.subscription(
      token,
      url,
      generation.toString(16).padStart(16, "0"),
      (enabled ? "a" : "b").repeat(64),
      "",
      enabled ? index : "[]",
      '[".except"]',
    );
  const display = (selector: string) =>
    win.getComputedStyle(win.document.querySelector(selector)!).display;
  return { win, api, token, url, update, display };
}

test("generic rules are installed only for observed tokens, including later DOM changes", async () => {
  const { win, update, display } = await fixture();
  expect(update(1, true)).toBe(true);
  await expect.poll(() => display(".ad")).toBe("none");
  expect(display(".except")).not.toBe("none");
  expect(display("main")).not.toBe("none");
  expect([...win.document.adoptedStyleSheets].flatMap((s) => [...s.cssRules]).length).toBe(1);
  const late = win.document.createElement("div");
  win.document.body.append(late);
  late.id = "late";
  await expect.poll(() => display("#late")).toBe("none");
  expect([...win.document.adoptedStyleSheets].flatMap((s) => [...s.cssRules]).length).toBe(2);
});

test("a mutation burst too large to track still gets scanned", async () => {
  const { win, update, display } = await fixture();
  expect(update(1, true)).toBe(true);
  await expect.poll(() => display(".ad")).toBe("none");
  // One observer callback with more records than are tracked one by one.
  for (let i = 0; i < 300; i++) win.document.body.append(win.document.createElement("p"));
  const late = win.document.createElement("div");
  late.id = "late";
  win.document.body.append(late);
  await expect.poll(() => display("#late")).toBe("none");
});

test("pause removes subscription rules and stale updates cannot re-enable them", async () => {
  const { win, update, display, api, token, url } = await fixture();
  expect(update(1, true)).toBe(true);
  await expect.poll(() => display(".ad")).toBe("none");
  expect(update(2, false)).toBe(true);
  expect(display(".ad")).not.toBe("none");
  expect(update(1, true)).toBe(false);
  const ad = win.document.createElement("div");
  ad.id = "late";
  win.document.body.append(ad);
  expect(display("#late")).not.toBe("none");
  expect(
    api.subscription(
      "0".repeat(32),
      url,
      "0000000000000003",
      "a".repeat(64),
      "body{display:none}",
      "[]",
      "[]",
    ),
  ).toBe(false);
  expect(
    api.subscription(
      token,
      `${url}#changed`,
      "0000000000000003",
      "a".repeat(64),
      "body{display:none}",
      "[]",
      "[]",
    ),
  ).toBe(false);
  expect(update(3, true)).toBe(true);
  await expect.poll(() => display("#late")).toBe("none");
});

test("unchanged subscription reuses its sheet and rejects removed sheets or stale generations", async () => {
  const { win, update, api, token, url, display } = await fixture();
  expect(update(1, true)).toBe(true);
  await expect.poll(() => display(".ad")).toBe("none");
  const sheet = win.document.adoptedStyleSheets[0];
  expect(JSON.parse(api.inspectEncoded()).subscription).toBe("a".repeat(64));
  expect(api.reuseSubscription(token, url, "0000000000000002", "a".repeat(64))).toBe(true);
  expect(win.document.adoptedStyleSheets[0]).toBe(sheet);
  expect(api.reuseSubscription(token, url, "0000000000000001", "a".repeat(64))).toBe(false);
  expect(api.reuseSubscription(token, url, "invalid", "a".repeat(64))).toBe(false);
  expect(api.reuseSubscription(token, url, "0000000000000003", "b".repeat(64))).toBe(false);
  win.document.adoptedStyleSheets = [];
  expect(JSON.parse(api.inspectEncoded()).subscription).toBeNull();
  expect(api.reuseSubscription(token, url, "0000000000000003", "a".repeat(64))).toBe(false);
  expect(update(3, true)).toBe(true);
  await expect.poll(() => display(".ad")).toBe("none");
});

test("host pulls wait for new tokens and late selectors reject stale batches", async () => {
  const { win, api, token, url, display } = await fixture();
  const pull = api as PullApi;
  const fingerprint = "d".repeat(64);
  expect(api.subscription(token, url, "0000000000000001", fingerprint, "", true, "")).toBe(true);
  const first = JSON.parse((await pull.pullGeneric(token, url, fingerprint))!) as TokenBatch;
  expect(first.tokens).toContain(".ad");
  expect(first.tokens).toContain(".except");
  expect(pull.applyGeneric(token, url, fingerprint, first.serial, [".ad"])).toBe(true);
  expect(display(".ad")).toBe("none");
  expect(display(".except")).not.toBe("none");
  let settled = false;
  const pending = Promise.resolve(pull.pullGeneric(token, url, fingerprint)).then((value) => {
    settled = true;
    return value;
  });
  await new Promise<void>((resolve) =>
    win.requestAnimationFrame(() => win.requestAnimationFrame(() => resolve())),
  );
  expect(settled).toBe(false);
  const late = win.document.createElement("div");
  late.id = "late";
  win.document.body.append(late);
  const second = JSON.parse((await pending)!) as TokenBatch;
  expect(second.tokens).toContain("#late");
  expect(second.tokens).not.toContain(".ad");
  expect(pull.applyGeneric(token, url, fingerprint, first.serial, ["#late"])).toBe(false);
  expect(pull.applyGeneric(token, url, fingerprint, second.serial, ["#late"])).toBe(true);
  expect(display("#late")).toBe("none");
  expect(display("main")).not.toBe("none");
});

test("pausing resolves an outstanding pull and prevents old selectors returning", async () => {
  const { api, token, url, display } = await fixture();
  const pull = api as PullApi;
  const fingerprint = "d".repeat(64);
  expect(api.subscription(token, url, "0000000000000001", fingerprint, "", true, "")).toBe(true);
  const batch = JSON.parse((await pull.pullGeneric(token, url, fingerprint))!) as TokenBatch;
  expect(pull.applyGeneric(token, url, fingerprint, batch.serial, [".ad"])).toBe(true);
  const pending = pull.pullGeneric(token, url, fingerprint);
  expect(api.subscription(token, url, "0000000000000002", "e".repeat(64), "", false, "")).toBe(
    true,
  );
  expect(await pending).toBe(null);
  expect(pull.applyGeneric(token, url, fingerprint, batch.serial, [".ad"])).toBe(false);
  expect(display(".ad")).not.toBe("none");
});

test("removing a node during token backpressure does not lose later ads", async () => {
  const { win, api, token, url, display } = await fixture();
  const pull = api as PullApi;
  for (let i = 0; i < 1000; i++) {
    const element = win.document.createElement("div");
    element.id = `queue-${i}`;
    win.document.body.append(element);
  }
  const late = win.document.createElement("div");
  late.id = "late";
  win.document.body.append(late);
  const fingerprint = "f".repeat(64);
  expect(api.subscription(token, url, "0000000000000001", fingerprint, "", true, "")).toBe(true);
  const first = JSON.parse((await pull.pullGeneric(token, url, fingerprint))!) as TokenBatch;
  const numbers = first.tokens
    .filter((t) => t.startsWith("#queue-"))
    .map((t) => Number(t.slice(7)));
  const last = Math.max(...numbers);
  // While the first batch awaits its acknowledgement, the next 256 tokens
  // fill the bounded queue and stop at exactly the following element.
  await new Promise<void>((resolve) => win.setTimeout(resolve, 300));
  win.document.getElementById(`queue-${last + 257}`)!.remove();
  expect(pull.applyGeneric(token, url, fingerprint, first.serial, [])).toBe(true);
  let found = false;
  for (let attempt = 0; attempt < 20 && !found; attempt++) {
    const batch = JSON.parse((await pull.pullGeneric(token, url, fingerprint))!) as TokenBatch;
    found = batch.tokens.includes("#late");
    expect(pull.applyGeneric(token, url, fingerprint, batch.serial, found ? ["#late"] : [])).toBe(
      true,
    );
  }
  expect(found).toBe(true);
  expect(display("#late")).toBe("none");
});
