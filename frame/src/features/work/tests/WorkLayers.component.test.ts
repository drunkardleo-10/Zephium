import { expect, test } from "vitest";
import { layerRelease } from "../lib/workspace/layers";

const wait = (ms: number) => new Promise((done) => setTimeout(done, ms));

function canvas() {
  const root = document.createElement("div");
  const viewport = document.createElement("div");
  viewport.className = "svelte-flow__viewport";
  const walker = document.createElement("div");
  viewport.append(walker);
  root.append(viewport);
  document.body.append(root);
  let released = 0;
  new MutationObserver((records) => {
    released += records.filter((record) => record.addedNodes.length).length;
  }).observe(viewport, { childList: true });
  return { root, walker, released: () => released };
}

test("the canvas lets its layer go once motion settles, never while a helper still moves", async () => {
  const { root, walker, released } = canvas();
  const release = layerRelease(root);
  const walk = walker.animate([{ translate: "0 0" }, { translate: "40px 0" }], {
    duration: 400,
    iterations: Infinity,
  });
  release.moved();
  await wait(700);
  expect(released()).toBe(0);
  walk.cancel();
  await wait(400);
  expect(released()).toBe(1);
  expect(root.querySelectorAll(".svelte-flow__viewport > *")).toHaveLength(1);

  const arrive = walker.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 300 });
  release.moved();
  await wait(200);
  expect(released()).toBe(1);
  await arrive.finished;
  await wait(400);
  expect(released()).toBe(2);

  release.destroy();
  release.moved();
  walker.dispatchEvent(new Event("transitionend", { bubbles: true }));
  await wait(400);
  expect(released()).toBe(2);
  root.remove();
});
