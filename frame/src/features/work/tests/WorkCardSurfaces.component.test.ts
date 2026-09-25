import "$styles/global.css";
import { afterEach, expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { favicons } from "$domain/favicons";
import { emitNativeEvent } from "$shared/testing/native-events";
import SubjectCard from "../components/cards/SubjectCard.svelte";
import SourcesCard from "../components/cards/SourcesCard.svelte";
import PageCard from "../components/cards/PageCard.svelte";
import ResultCard from "../components/cards/ResultCard.svelte";
import { canvasOpen } from "../lib/canvas-context";
import type { CanvasItem } from "../lib/canvas-model";

const base = { kind: "", detail: "", status: "" };

/** One opaque colour across a 32 px raster, as native delivers a site's mark. */
function solid(red: number, green: number, blue: number): string {
  let binary = "";
  for (let index = 0; index < 32 * 32; index += 1)
    binary += String.fromCharCode(red, green, blue, 255);
  return btoa(binary);
}

async function marks(...origins: string[]) {
  await favicons.init();
  emitNativeEvent("favicons", {
    surface: "chrome",
    profile_id: "profile",
    entries: origins.map((origin) => ({ origin, revision: "a", rgba: solid(220, 60, 70) })),
  });
}

afterEach(() => favicons.dispose());

test("a subject shows its hero picture, or else its site's mark beside the name, never an initial", async () => {
  await page.viewport(1200, 800);
  await marks("https://www.airbnb.com");
  const subject: CanvasItem = {
    ...base,
    id: "subject",
    type: "subject",
    title: "Charming Cole Valley Victorian",
    subject: { name: "Charming Cole Valley Victorian", homepage: "https://www.airbnb.com/rooms/1" },
    facts: [{ label: "Price per month", value: "$4,120" }],
  };
  const plain = await render(SubjectCard, { item: subject, selected: false });
  expect(plain.container.querySelector(".hero")).toBeNull();
  const header = plain.container.querySelector("header")!;
  expect(header.querySelector(".leading .favicon canvas")).not.toBeNull();
  expect(header.textContent?.trim()).toBe(subject.title);
  expect(plain.container.querySelector("[data-card-id='subject']")).not.toBeNull();
  await plain.unmount();

  const pictured = await render(SubjectCard, {
    item: { ...subject, image: { profile: "profile", digest: "d".repeat(64) } },
    selected: false,
  });
  expect(pictured.container.querySelector(".hero .picture")).not.toBeNull();
  expect(pictured.container.querySelector("header .leading")).toBeNull();
  await expect.element(pictured.getByText("airbnb.com")).toBeVisible();
  await pictured.unmount();
});

test("a sources card draws a strip of real site marks and six rows of mark, host and title", async () => {
  await page.viewport(1200, 800);
  await marks("https://a.example", "https://b.example");
  const hosts = ["a.example", "b.example", "c.example", "a.example", "b.example", "c.example"];
  const item: CanvasItem = {
    ...base,
    id: "sources",
    type: "sources",
    title: "7 sources",
    sources: [...hosts, "d.example"].map((where, index) => ({
      key: `source-${index}`,
      url: `https://${where}/page-${index}`,
      where,
      title: `Page ${index + 1}`,
    })),
  };
  const screen = await render(SourcesCard, { item, selected: false });
  // One mark per site: two real favicons, and a neutral stand-in where the cache has none.
  expect(screen.container.querySelectorAll(".strip .mark")).toHaveLength(4);
  expect(screen.container.querySelectorAll(".strip .favicon canvas")).toHaveLength(2);
  const rows = [...screen.container.querySelectorAll(".rows li")];
  expect(rows).toHaveLength(6);
  expect(rows[0]!.querySelector(".favicon canvas")).not.toBeNull();
  expect(rows[0]!.querySelector(".where")?.textContent).toBe("a.example");
  expect(rows[0]!.querySelector(".title")?.textContent).toBe("Page 1");
  await expect.element(screen.getByText("7 sources")).toBeVisible();
  await screen.unmount();
});

test("a page being read shows the loading arc in place of its mark, and the mark once read", async () => {
  await page.viewport(1200, 800);
  const item: CanvasItem = {
    ...base,
    id: "page",
    type: "page",
    title: "shop.example",
    detail: "https://shop.example/p/1",
    status: "Reading",
    page: { url: "https://shop.example/p/1", host: "shop.example", frame: null, live: true },
  };
  const screen = await render(PageCard, { item, selected: false });
  const mark = () => screen.container.querySelector<HTMLElement>(".about .favicon")!;
  expect(mark().dataset.loading).toBe("true");
  await expect.element(screen.getByText("shop.example", { exact: true })).toBeVisible();
  // Nothing after the host: no status word, no reader mark.
  expect(screen.container.textContent).not.toContain("Reading");
  await screen.rerender({
    item: { ...item, page: { ...item.page!, live: false } },
    selected: false,
  });
  expect(mark().dataset.loading).toBe("false");
  await screen.unmount();
});

test("a result is a cover: its kind, the whole summary, section rows, and a click opens it", async () => {
  await page.viewport(1200, 800);
  const lead = "Stay in Alfama and take the 28 tram early. ".repeat(5).trim();
  const onopen = vi.fn();
  const item: CanvasItem = {
    ...base,
    id: "result",
    type: "result",
    title: "Weekend in Lisbon",
    artifact: {
      key: "artifact",
      title: "Weekend in Lisbon",
      reviewLabel: "Done",
      evidence: [],
      content: {
        kind: "document",
        paragraphs: [
          "Summary",
          lead,
          "## Where to stay",
          "Alfama is quiet.",
          "## Getting around",
          "Walk.",
        ],
      },
    },
  };
  const screen = await render(ResultCard, {
    props: { id: "result", item, selected: false, onaction() {} },
    context: new Map([[canvasOpen, onopen]]),
  });
  await expect.element(screen.getByText("Document", { exact: true })).toBeVisible();
  const summary = screen.getByText(lead).element() as HTMLElement;
  expect(summary.scrollHeight).toBeLessThanOrEqual(summary.clientHeight + 1);
  const rows = [...screen.container.querySelectorAll(".sections li")];
  expect(rows.map((row) => row.textContent)).toEqual(["Where to stay", "Getting around"]);
  await screen.getByText("Getting around").click();
  expect(onopen).toHaveBeenCalledExactlyOnceWith("result");
  await screen.unmount();
});

test("a chart result draws its plot in the card, without the basis line", async () => {
  await page.viewport(1200, 800);
  const item: CanvasItem = {
    ...base,
    id: "chart",
    type: "result",
    title: "Nightly price",
    artifact: {
      key: "chart",
      title: "Nightly price",
      reviewLabel: "Done",
      evidence: [],
      content: {
        kind: "chart",
        xLabel: "Month",
        yLabel: "Price",
        series: [
          {
            name: "Alfama",
            points: [
              { label: "Jan", value: "120" },
              { label: "Feb", value: "135" },
              { label: "Mar", value: "150" },
            ],
          },
        ],
        basis: { method: "Listed nightly rate" },
      },
    },
  };
  const screen = await render(ResultCard, { id: "chart", item, selected: false, onaction() {} });
  await expect.element(screen.getByText("Chart", { exact: true })).toBeVisible();
  await expect.element(screen.getByRole("img", { name: /Month; Price/ })).toBeVisible();
  expect(screen.container.querySelectorAll("svg .bar")).toHaveLength(3);
  expect(screen.container.textContent).not.toContain("Listed nightly rate");
  await screen.unmount();
});
