import "$styles/global.css";
import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import FindingsCard from "../components/cards/FindingsCard.svelte";
import ResultCard from "../components/cards/ResultCard.svelte";
import CompareCard from "../components/cards/CompareCard.svelte";
import FileCard from "../components/cards/FileCard.svelte";
import SubjectCard from "../components/cards/SubjectCard.svelte";
import { defaultSize, type CanvasItem } from "../lib/canvas-model";

const base = { kind: "", detail: "", status: "Done" };

test("a findings card lists eight claims with their confidence, then how many more", async () => {
  await page.viewport(1200, 800);
  const confidence = [
    "supported",
    "contradicted",
    "inferred",
    "unverified",
    "supported",
    "supported",
    "supported",
    "inferred",
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
      total: 10,
    },
  };
  const screen = await render(FindingsCard, { item, selected: false });
  const rows = [...screen.container.querySelectorAll("li")];
  expect(rows.map((row) => row.className.split(" ")[0])).toEqual(confidence.slice(0, 8));
  const dot = (row: Element) => getComputedStyle(row.querySelector(".dot")!).backgroundColor;
  expect(dot(rows[0]!)).not.toBe(dot(rows[1]!));
  expect(dot(rows[2]!)).toBe(dot(rows[3]!));
  await expect.element(screen.getByText("Tower Bridge")).toBeVisible();
  await expect.element(screen.getByText("Claim 8", { exact: true })).toBeVisible();
  await expect.element(screen.getByText("+2 more")).toBeVisible();
  expect(screen.container.textContent).not.toContain("Claim 9");
  await screen.unmount();
});

test("a document result shows its whole summary and its sections, never its steps or chips", async () => {
  await page.viewport(1200, 800);
  const lead = "Stay in Alfama and take the 28 tram early. ".repeat(6).trim();
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
          lead,
          "## Where to stay",
          "Alfama is quiet in the morning.",
          "Next steps:",
          "- Book the hotel\n- Buy a Viva Viagem card\n- Reserve Belém tickets\n- Pack",
        ],
      },
    },
  };
  const screen = await render(ResultCard, { id: "result", item, selected: false, onaction() {} });
  const summary = screen.getByText(lead);
  await expect.element(summary).toBeVisible();
  // Whole: the paragraph is not cut to a few lines.
  const shown = summary.element() as HTMLElement;
  expect(shown.scrollHeight).toBeLessThanOrEqual(shown.clientHeight + 1);
  await expect.element(screen.getByText("Where to stay")).toBeVisible();
  // The steps stand beside the card as cards of their own.
  expect(screen.container.textContent).not.toContain("Book the hotel");
  expect(screen.container.querySelector(".chip")).toBeNull();
  expect(screen.container.textContent).not.toContain("visitlisboa.com");
  // State belongs to the agent line: no status word in a footer.
  expect(screen.container.querySelector("footer")).toBeNull();
  await screen.unmount();
});

test("a subject card shows four facts whole, the lead one first", async () => {
  await page.viewport(1200, 800);
  const facts = [
    { label: "Price per month", value: "$4,120" },
    { label: "Rating", value: "4.89 from 152 reviews" },
    { label: "Displayed routing", value: "Walk to Golden Gate Park, then the N Judah into town" },
    { label: "Workspace", value: "Dedicated desk" },
  ];
  const item: CanvasItem = {
    ...base,
    id: "subject",
    type: "subject",
    title: "Charming Cole Valley Victorian",
    subject: { name: "Charming Cole Valley Victorian", homepage: "https://www.airbnb.com/rooms/1" },
    facts,
  };
  const size = defaultSize(item);
  const screen = await render(SubjectCard, { item, selected: false });
  const card = screen.container.querySelector<HTMLElement>(".card")!;
  card.parentElement!.style.inlineSize = `${size.width}px`;
  card.parentElement!.style.blockSize = `${size.height}px`;
  for (const fact of facts) {
    await expect.element(screen.getByText(fact.label, { exact: true })).toBeVisible();
    const value = screen.getByText(fact.value, { exact: true }).element() as HTMLElement;
    expect(value.scrollHeight).toBeLessThanOrEqual(value.clientHeight + 1);
    expect(getComputedStyle(value).textOverflow).not.toBe("ellipsis");
  }
  // No picture yet: the site's mark stands beside the name, not a hero placeholder or an initial.
  expect(screen.container.querySelector(".hero")).toBeNull();
  expect(screen.container.querySelector("header .leading .favicon")).not.toBeNull();
  await expect.element(screen.getByText("airbnb.com")).toBeVisible();
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

test("a yes/no row reads as a check or a faint cross, centred; unknown stays a dash", async () => {
  await page.viewport(1200, 800);
  const cell = (value: object) => ({ value, evidence: [], generalKnowledge: false });
  const item: CanvasItem = {
    ...base,
    id: "compare",
    type: "result",
    title: "Stays compared",
    artifact: {
      key: "artifact",
      title: "Stays compared",
      reviewLabel: "Done",
      evidence: [],
      content: {
        kind: "matrix",
        subjects: [{ name: "Cole Valley" }, { name: "Potrero" }, { name: "Mission" }],
        criteria: [
          { name: "Workspace", kind: "presence" },
          { name: "Wi-Fi", kind: "text" },
        ],
        cells: [
          [cell({ kind: "presence", present: true }), cell({ kind: "text", text: "Yes" })],
          [cell({ kind: "presence", present: false }), cell({ kind: "text", text: "No" })],
          [cell({ kind: "unknown" }), cell({ kind: "unknown" })],
        ] as never,
        notes: [],
      },
    },
  };
  const screen = await render(CompareCard, { item, selected: false });
  const rows = [...screen.container.querySelectorAll(".row")];
  expect(rows).toHaveLength(2);
  for (const row of rows) {
    const cells = [...row.querySelectorAll("dd")];
    expect(cells.every((cell) => cell.classList.contains("check"))).toBe(true);
    expect(getComputedStyle(cells[0]!).textAlign).toBe("center");
    expect(cells[0]!.querySelector(".glyph.yes")?.getAttribute("aria-label")).toBe("Yes");
    const no = cells[1]!.querySelector(".glyph")!;
    expect(no.classList.contains("yes")).toBe(false);
    expect(no.getAttribute("aria-label")).toBe("No");
    expect(getComputedStyle(no).color).not.toBe(
      getComputedStyle(cells[0]!.querySelector(".glyph")!).color,
    );
    expect(cells[2]!.querySelector(".dash")).not.toBeNull();
  }
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
