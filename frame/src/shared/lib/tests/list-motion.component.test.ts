import { afterEach, beforeEach, expect, test } from "vitest";
import "$styles/global.css";
import { ListMotion } from "../list-motion";

let root: HTMLElement;

beforeEach(() => {
  document.documentElement.dataset.reduceMotion = "false";
  root = document.createElement("div");
  document.body.append(root);
});

afterEach(() => {
  root.remove();
  for (const stray of document.body.querySelectorAll("[aria-hidden='true']")) stray.remove();
});

function list(keys: string[]): HTMLUListElement {
  const container = document.createElement("ul");
  container.style.cssText =
    "display:flex;flex-direction:column;gap:4px;width:200px;margin:0;padding:0";
  for (const key of keys) container.append(item(key));
  root.append(container);
  return container;
}

function item(key: string): HTMLLIElement {
  const element = document.createElement("li");
  element.dataset.motionKey = key;
  element.dataset.zephiumTabId = key;
  element.style.cssText = "height:36px;list-style:none";
  const mark = document.createElement("span");
  mark.dataset.favicon = "";
  mark.dataset.zephiumTabLabel = "";
  mark.style.cssText = "display:block;width:16px;height:16px";
  element.append(mark);
  return element;
}

const microtask = () => new Promise((resolve) => queueMicrotask(() => resolve(undefined)));
const byKey = (container: HTMLElement, key: string) =>
  container.querySelector<HTMLElement>(`:scope > [data-motion-key="${key}"]`)!;

test("a departed item returns only as an anonymous, unreachable ghost", async () => {
  const container = list(["a", "b", "c"]);
  const motion = new ListMotion();
  motion.capture(container);
  byKey(container, "b").remove();
  motion.play(container);
  await microtask();

  const ghost = container.querySelector<HTMLElement>(":scope > [aria-hidden='true']");
  expect(ghost).not.toBeNull();
  expect(ghost!.inert).toBe(true);
  // Native resolves tabs through these; a departed tab must never be found.
  expect(ghost!.querySelector("[data-zephium-tab-label]")).toBeNull();
  expect(ghost!.hasAttribute("data-zephium-tab-id")).toBe(false);
  expect(ghost!.hasAttribute("data-motion-key")).toBe(false);
  expect(container.querySelectorAll("[data-zephium-tab-id]")).toHaveLength(2);

  // The item after it closes the gap rather than jumping.
  expect(byKey(container, "c").getAnimations()).not.toHaveLength(0);

  await expect.poll(() => ghost!.isConnected, { timeout: 2000 }).toBe(false);
});

test("an item that moves slides from where it was, and nothing else animates", async () => {
  const container = list(["a", "b", "c"]);
  const motion = new ListMotion();
  motion.capture(container);
  container.prepend(byKey(container, "c"));
  motion.play(container);

  expect(byKey(container, "c").getAnimations()).toHaveLength(1);
  expect(byKey(container, "a").getAnimations()).toHaveLength(1);
  expect(byKey(container, "b").getAnimations()).toHaveLength(1);

  // A change that moves nothing costs nothing.
  for (const element of container.children) {
    for (const animation of element.getAnimations()) animation.finish();
  }
  motion.capture(container);
  motion.play(container);
  for (const element of container.children) expect(element.getAnimations()).toHaveLength(0);
});

test("an item that crosses lists carries its mark with it", async () => {
  const from = list(["a", "b"]);
  const to = list(["x"]);
  const source = new ListMotion();
  const target = new ListMotion({ enter: () => "grow" });
  source.capture(from);
  target.capture(to);
  byKey(from, "a").remove();
  to.append(item("a"));
  source.play(from);
  target.play(to);
  await microtask();

  const flyer = [...document.body.children].find(
    (element) => element instanceof HTMLElement && element.style.position === "fixed",
  );
  expect(flyer).toBeDefined();
  expect(flyer!.getAnimations()).toHaveLength(1);
  // The landing mark waits for the one in flight.
  expect(byKey(to, "a").querySelector<HTMLElement>("[data-favicon]")!.style.visibility).toBe(
    "hidden",
  );
  await expect.poll(() => flyer!.isConnected, { timeout: 2000 }).toBe(false);
  expect(byKey(to, "a").querySelector<HTMLElement>("[data-favicon]")!.style.visibility).toBe("");
});

test("a list replaced wholesale arrives without ceremony", async () => {
  const container = list(["a", "b"]);
  const motion = new ListMotion();
  motion.capture(container);
  container.replaceChildren(item("c"), item("d"));
  motion.play(container);
  await microtask();
  expect(container.querySelector("[aria-hidden='true']")).toBeNull();
  for (const element of container.children) expect(element.getAnimations()).toHaveLength(0);
});

test("reduced motion changes the list and animates nothing", async () => {
  document.documentElement.dataset.reduceMotion = "true";
  const container = list(["a", "b", "c"]);
  const motion = new ListMotion();
  motion.capture(container);
  byKey(container, "a").remove();
  motion.play(container);
  await microtask();
  expect(container.querySelector("[aria-hidden='true']")).toBeNull();
  for (const element of container.children) expect(element.getAnimations()).toHaveLength(0);
});
