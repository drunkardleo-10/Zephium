import "$styles/global.css";
import { createRawSnippet } from "svelte";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import Lift from "../components/Lift.svelte";

test("the lift grows out of the card it names and folds back into it on Escape", async () => {
  await page.viewport(1000, 760);
  const card = document.createElement("article");
  card.dataset.cardId = "result:1";
  Object.assign(card.style, {
    position: "fixed",
    left: "80px",
    top: "90px",
    width: "240px",
    height: "140px",
    background: "var(--color-surface)",
  });
  document.body.append(card);
  const onclose = vi.fn();
  const screen = await render(Lift, {
    origin: card.getBoundingClientRect(),
    source: "result:1",
    bounds: new DOMRect(0, 0, 1000, 760),
    title: "Weekend in Lisbon",
    onclose,
    children: createRawSnippet(() => ({ render: () => "<p>The whole document.</p>" })),
  });
  const lift = screen.getByRole("dialog", { name: "Weekend in Lisbon" });
  await expect.element(lift).toBeVisible();
  await expect.element(screen.getByText("The whole document.")).toBeVisible();
  // While it stands as the lift, the card is not drawn twice.
  await expect.poll(() => card.style.visibility).toBe("hidden");
  await userEvent.keyboard("{Escape}");
  await expect.poll(() => onclose.mock.calls.length, { timeout: 2000 }).toBe(1);
  expect(card.style.visibility).toBe("");
  expect(card.style.viewTransitionName).toBe("");
  expect(document.documentElement.className).not.toMatch(/work-lift/u);
  await screen.unmount();
  card.remove();
});

test("a lift whose card is no longer drawn opens and closes in place", async () => {
  await page.viewport(1000, 760);
  const onclose = vi.fn();
  const screen = await render(Lift, {
    origin: new DOMRect(10, 10, 200, 100),
    source: "gone",
    bounds: new DOMRect(0, 0, 1000, 760),
    title: "Sources",
    onclose,
    children: createRawSnippet(() => ({ render: () => "<p>Rows.</p>" })),
  });
  await expect.element(screen.getByRole("dialog", { name: "Sources" })).toBeVisible();
  await screen.getByRole("button", { name: "Back to canvas" }).click();
  await expect.poll(() => onclose.mock.calls.length, { timeout: 2000 }).toBe(1);
  await screen.unmount();
});
