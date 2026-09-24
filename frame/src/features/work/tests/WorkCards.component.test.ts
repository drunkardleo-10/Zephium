import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import FindingsCard from "../components/cards/FindingsCard.svelte";
import ResultCard from "../components/cards/ResultCard.svelte";
import CompareCard from "../components/cards/CompareCard.svelte";
import FileCard from "../components/cards/FileCard.svelte";
import type { CanvasItem } from "../lib/canvas-model";

const base = { kind: "", detail: "", status: "Done" };

test("a findings card lists four claims with their confidence, then how many more", async () => {
  await page.viewport(1200, 800);
  const confidence = [
    "supported",
    "contradicted",
    "inferred",
    "unverified",
    "supported",
    "supported",
  ];
  const item: CanvasItem = {
    ...base,
    id: "findings",
    type: "findings",
    title: "Findings",
    findings: {
      items: confidence.map((level, index) => ({
        claim: `Claim ${index + 1}`,
        confidence: level as "supported",
        evidence: 1,
        ...(index === 0 ? { subject: "Tower Bridge" } : {}),
      })),
      total: 6,
    },
  };
  const screen = await render(FindingsCard, { item, selected: false });
  const rows = [...screen.container.querySelectorAll("li")];
  expect(rows.map((row) => row.className.split(" ")[0])).toEqual([
    "supported",
    "contradicted",
    "inferred",
    "unverified",
  ]);
  const dot = (row: Element) => getComputedStyle(row.querySelector(".dot")!).backgroundColor;
  expect(dot(rows[0]!)).not.toBe(dot(rows[1]!));
  expect(dot(rows[2]!)).toBe(dot(rows[3]!));
  await expect.element(screen.getByText("Tower Bridge")).toBeVisible();
  await expect.element(screen.getByText("+2 more")).toBeVisible();
  expect(screen.container.textContent).not.toContain("Claim 5");
  await screen.unmount();
});

test("a document result shows its lead and next steps, never citation chips", async () => {
  await page.viewport(1200, 800);
  const item: CanvasItem = {
    ...base,
    id: "result",
    type: "result",
    title: "Weekend in Lisbon",
    artifact: {
      key: "artifact",
      title: "Weekend in Lisbon",
      reviewLabel: "Done",
      evidence: [{ key: "e", label: "Visit Lisboa", origin: "visitlisboa.com" }],
      content: {
        kind: "document",
        paragraphs: [
          "Summary",
          "Stay in Alfama and take the 28 tram early.",
          "Background that the card leaves to the lift.",
          "Next steps:",
          "- Book the hotel\n- Buy a Viva Viagem card\n- Reserve Belém tickets\n- Pack",
        ],
      },
    },
  };
  const screen = await render(ResultCard, { id: "result", item, selected: false, onaction() {} });
  await expect
    .element(screen.getByText("Stay in Alfama and take the 28 tram early."))
    .toBeVisible();
  await expect.element(screen.getByText("Book the hotel")).toBeVisible();
  expect(screen.container.textContent).not.toContain("Background that the card leaves");
  expect(screen.container.textContent).not.toContain("Pack");
  expect(screen.container.querySelector(".chip")).toBeNull();
  expect(screen.container.textContent).not.toContain("visitlisboa.com");
  await screen.unmount();
});

test("a comparison card shows three subjects and a stub for the rest", async () => {
  await page.viewport(1200, 800);
  const names = ["Alpha", "Beta", "Gamma", "Delta", "Epsilon"];
  const item: CanvasItem = {
    ...base,
    id: "compare",
    type: "result",
    title: "Sets compared",
    artifact: {
      key: "artifact",
      title: "Sets compared",
      reviewLabel: "Done",
      evidence: [],
      content: {
        kind: "matrix",
        subjects: names.map((name) => ({ name })),
        criteria: [
          { name: "Price", kind: "text" },
          { name: "Pieces", kind: "measurement", unit: "pcs" },
        ],
        cells: names.map((_, index) => [
          {
            value: { kind: "money", amount: String(100 + index), currency: "USD" },
            evidence: [],
            generalKnowledge: false,
          },
          {
            value: { kind: "measurement", value: String(1000 + index) },
            evidence: [],
            generalKnowledge: false,
          },
        ]),
        notes: [],
      },
    },
  };
  const screen = await render(CompareCard, { item, selected: false });
  const shown = [...screen.container.querySelectorAll(".subject .name")].map((n) => n.textContent);
  expect(shown).toEqual(["Alpha", "Beta", "Gamma"]);
  await expect.element(screen.getByText("+2", { exact: true })).toBeVisible();
  expect(screen.container.textContent).not.toContain("Delta");
  await screen.unmount();
});

test("a changed file shows its line counts", async () => {
  await page.viewport(1200, 800);
  const item: CanvasItem = {
    ...base,
    id: "file",
    type: "file",
    title: "plan.md",
    file: {
      name: "plan.md",
      folder: "/Users/reader/notes",
      what: "changed",
      delta: { plus: 12, minus: 3 },
    },
  };
  const screen = await render(FileCard, { item, selected: false });
  expect(screen.container.querySelector(".delta")?.textContent?.replace(/\s+/gu, " ")).toBe(
    "+12 −3",
  );
  await expect.element(screen.getByText("Changed")).toBeVisible();
  await expect.element(screen.getByText("~/notes")).toBeVisible();
  await screen.unmount();
});
