import { afterEach, expect, inject, test } from "vitest";

declare module "vitest" {
  export interface ProvidedContext {
    contentStyleSource: string;
  }
}
type StyleApi = {
  inspect(): { token: string; url: string };
  subscription(
    token: string,
    url: string,
    generation: string,
    fingerprint: string,
    css: string,
    index: string,
    exceptions: string,
  ): boolean;
};
let frame: HTMLIFrameElement | undefined;
afterEach(() => {
  frame?.remove();
  frame = undefined;
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
