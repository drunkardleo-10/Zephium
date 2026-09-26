import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import SourcesCard from "../components/cards/SourcesCard.svelte";
import { defaultSize, type CanvasItem } from "../lib/canvas-model";

const unread = [
  {
    key: "page:run:b",
    url: "https://boards.greenhouse.io/acme",
    host: "boards.greenhouse.io",
    note: "The page gave nothing",
  },
  {
    key: "page:run:d",
    url: "https://jobs.ashbyhq.com/acme",
    host: "jobs.ashbyhq.com",
    note: "An action on the page did not work",
  },
];

/** The card at the size the canvas gives it before it renders. */
async function sized(item: CanvasItem) {
  const screen = await render(SourcesCard, { item, selected: false });
  const size = defaultSize(item);
  screen.container.style.width = `${size.width}px`;
  screen.container.style.height = `${size.height}px`;
  return screen;
}

test("pages that could not be read are one quiet line in Sources that lists them inside the card", async () => {
  await page.viewport(1200, 800);
  const item: CanvasItem = {
    id: "sources",
    type: "sources",
    kind: "Sources",
    title: "2 sources",
    detail: "",
    status: "",
    sources: ["a.example", "b.example"].map((where, index) => ({
      key: `source-${index}`,
      url: `https://${where}/page`,
      where,
      title: `Page ${index + 1}`,
    })),
    unread,
  };
  const screen = await sized(item);
  const line = screen.getByRole("button", { name: "2 pages could not be read", exact: true });
  await expect.element(line).toBeVisible();
  await expect.element(line).toHaveAttribute("aria-expanded", "false");
  // The line sits inside the card the canvas sized, under the rows it keeps.
  const card = screen.container.querySelector("article")!.getBoundingClientRect();
  const footer = screen.container.querySelector("footer")!.getBoundingClientRect();
  expect(footer.bottom).toBeLessThanOrEqual(card.bottom + 0.5);
  expect(screen.container.querySelectorAll(".rows li")).toHaveLength(2);
  expect(screen.container.textContent).not.toContain("greenhouse");
  await line.click();
  await expect.element(line).toHaveAttribute("aria-expanded", "true");
  const list = screen.getByRole("list", { name: "Pages that could not be read" });
  const rows = [...screen.container.querySelectorAll(".rows.unread li")];
  expect(
    rows.map((row) => [
      row.querySelector(".where")?.textContent,
      row.querySelector(".title")?.textContent,
      row.children.length,
    ]),
  ).toEqual([
    ["boards.greenhouse.io", "The page gave nothing", 3],
    ["jobs.ashbyhq.com", "An action on the page did not work", 3],
  ]);
  await expect.element(list).toBeVisible();
  for (const row of rows)
    expect(row.getBoundingClientRect().bottom).toBeLessThanOrEqual(card.bottom + 0.5);
  await line.click();
  expect(screen.container.querySelector(".rows.unread")).toBeNull();
  await screen.unmount();
});

test("a run that only failed to read still has a Sources card, and one page reads singular", async () => {
  await page.viewport(1200, 800);
  const item: CanvasItem = {
    id: "sources",
    type: "sources",
    kind: "Sources",
    title: "Sources",
    detail: "",
    status: "",
    sources: [],
    unread: unread.slice(0, 1),
  };
  const screen = await sized(item);
  await expect
    .element(screen.getByRole("button", { name: "1 page could not be read", exact: true }))
    .toBeVisible();
  // Without unread pages the card is the size it always was.
  const { unread: _, ...plain } = item;
  expect(defaultSize(item).height).toBeGreaterThan(defaultSize(plain).height);
  await screen.unmount();
});
