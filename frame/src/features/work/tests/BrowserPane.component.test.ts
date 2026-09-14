import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import { tabFixture, revision } from "$shared/testing/fixtures";
import BrowserPane from "../components/pane/BrowserPane.svelte";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings();
});

test("the pane renders the presentation sentinels, measures its hole, and dismisses on Escape", async () => {
  await page.viewport(1200, 800);
  const tab = tabFixture({
    id: "pane-tab",
    title: "Docs",
    url: "https://docs.example/guide",
    projection_revision: revision(7),
    can_go_back: true,
  });
  const onmeasure = vi.fn();
  const onclose = vi.fn();
  const onopenbrowse = vi.fn();
  const onnavigate = vi.fn();
  const onback = vi.fn();
  const bounds = new DOMRect(0, 52, 1200, 740);
  const screen = await render(BrowserPane, {
    tab,
    applied: null,
    bounds,
    origin: null,
    phase: "opening",
    onmeasure,
    onnavigate,
    onback,
    onforward: vi.fn(),
    onreload: vi.fn(),
    onopenbrowse,
    onadd: vi.fn(),
    onclose,
  });
  const row = screen.container.querySelector<HTMLElement>("[data-zephium-tab-id]")!;
  expect(row.dataset.zephiumTabId).toBe("pane-tab");
  expect(row.dataset.zephiumTabUrl).toBe("https://docs.example/guide");
  expect(row.dataset.zephiumProjectionRevision).toBe(revision(7));
  expect(row.querySelector("[data-zephium-tab-label]")?.textContent).toBe("Docs");
  const address = screen.container.querySelector<HTMLInputElement>("[data-zephium-address]")!;
  expect(address.value).toBe("docs.example");

  await expect.poll(() => onmeasure.mock.calls.length).toBeGreaterThan(0);
  const hole = onmeasure.mock.lastCall![0];
  expect(hole.width).toBeGreaterThanOrEqual(480);
  expect(hole.height).toBeGreaterThanOrEqual(320);
  expect(hole.x).toBeGreaterThanOrEqual(bounds.left);
  expect(hole.y).toBeGreaterThan(bounds.top);
  await expect.element(screen.getByText("Opening…")).toBeVisible();

  await screen.getByRole("button", { name: "Back", exact: true }).click();
  expect(onback).toHaveBeenCalledOnce();
  await screen.getByRole("button", { name: "Forward", exact: true }).click({ force: true });

  address.focus();
  await expect.poll(() => address.value).toBe("https://docs.example/guide");
  await userEvent.fill(address, "github.com");
  await userEvent.keyboard("{Enter}");
  expect(onnavigate).toHaveBeenCalledExactlyOnceWith("github.com");

  address.focus();
  await userEvent.keyboard("{Escape}");
  expect(onclose).not.toHaveBeenCalled();
  await expect.poll(() => address.value).toBe("docs.example");

  await screen.getByRole("button", { name: "Open in Browse", exact: true }).click();
  expect(onopenbrowse).toHaveBeenCalledOnce();

  await userEvent.keyboard("{Escape}");
  expect(onclose).toHaveBeenCalledOnce();
  expect(screen.container.querySelector("[data-zephium-tab-id]")).toBeNull();
  await screen.unmount();
});

test("the pane follows the applied hole and reports geometry after a drag", async () => {
  await page.viewport(1200, 800);
  const tab = tabFixture({ id: "pane-tab", title: "Docs" });
  const onmeasure = vi.fn();
  const bounds = new DOMRect(0, 52, 1200, 740);
  const screen = await render(BrowserPane, {
    tab,
    applied: {
      tab: "pane-tab",
      x: 101,
      y: 140,
      width: 600,
      height: 400,
      presented: true,
      generation: 3,
    },
    bounds,
    origin: null,
    phase: "shown",
    onmeasure,
    onnavigate: vi.fn(),
    onback: vi.fn(),
    onforward: vi.fn(),
    onreload: vi.fn(),
    onopenbrowse: vi.fn(),
    onadd: vi.fn(),
    onclose: vi.fn(),
  });
  const pane = screen.container.querySelector<HTMLElement>(".pane")!;
  await expect.poll(() => pane.style.left).toBe("100px");
  expect(pane.style.inlineSize).toBe("602px");
  expect(screen.container.querySelector(".placeholder")).toBeNull();
  await expect.poll(() => onmeasure.mock.calls.length).toBeGreaterThan(0);
  const before = onmeasure.mock.lastCall![0];
  const head = screen.container.querySelector<HTMLElement>(".head")!;
  const rect = head.getBoundingClientRect();
  const pointer = (type: string, x: number, y: number) =>
    head.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        pointerId: 1,
        button: 0,
        clientX: x,
        clientY: y,
      }),
    );
  pointer("pointerdown", rect.left + 60, rect.top + 20);
  pointer("pointermove", rect.left + 100, rect.top + 50);
  pointer("pointerup", rect.left + 100, rect.top + 50);
  await expect.poll(() => onmeasure.mock.lastCall![0].x).toBe(before.x + 40);
  expect(onmeasure.mock.lastCall![0].y).toBe(before.y + 30);
  await screen.unmount();
});
