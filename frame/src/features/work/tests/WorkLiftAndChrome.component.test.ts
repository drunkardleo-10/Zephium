import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import type { WorkPageV1, WorkRuntimeProjection } from "$shared/ipc/bindings";
import Table from "$shared/ui/data/Artifact/Table.svelte";
import Matrix from "$shared/ui/data/Artifact/Matrix.svelte";
import Findings from "$shared/ui/data/Artifact/Findings.svelte";
import WorkSubjectInspector from "../components/WorkSubjectInspector.svelte";
import FindingsLift from "../components/FindingsLift.svelte";
import Compare from "../components/compare/Compare.svelte";
import ObjectiveCard from "../components/cards/ObjectiveCard.svelte";
import { compareModel } from "../lib/compare";
import { requestSize } from "../lib/card-size";
import { projection as base } from "./environment-fixtures";

// A frame that loads: one transparent pixel stands in for the page the run captured.
const PIXEL =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
vi.mock("$domain/resources", async (original) => ({
  ...(await original<typeof import("$domain/resources")>()),
  pageFrameUrl: () => PIXEL,
}));

function subjectState(): WorkRuntimeProjection {
  const projection: WorkRuntimeProjection = structuredClone(base);
  const execution = projection.executions[0]!;
  execution.user_artifacts = [];
  execution.artifacts = [
    {
      ...execution.artifacts[0]!,
      id: "artifact",
      data: {
        kind: "comparison_matrix",
        subjects: [{ name: "Stripe", homepage: "https://stripe.com" }],
        criteria: [{ name: "Founded", kind: { kind: "text" } }],
        cells: [[{ value: { kind: "text", text: "2010" }, evidence: [] }]],
        notes: [],
      },
    },
  ];
  return projection;
}

test("a subject without a picture opens on the frame of the page the run read", async () => {
  await page.viewport(1100, 800);
  const projection = subjectState();
  const execution = projection.executions[0]!;
  const read: WorkPageV1 = {
    execution: execution.id,
    attempt: "attempt",
    step: "step",
    url: "https://stripe.com/",
    live: false,
    frame: { generation: 1, width: 1280, height: 800 },
  };
  const onask = vi.fn();
  const screen = await render(WorkSubjectInspector, {
    reference: {
      kind: "subject",
      objective: projection.work.id,
      execution: execution.id,
      artifact: "artifact",
      index: 0,
    },
    objectives: new Map([[projection.work.id, projection]]),
    pages: [read],
    onask,
  });
  await expect.element(screen.getByRole("heading", { name: "Stripe" })).toBeVisible();
  const hero = screen.container.querySelector<HTMLElement>("figure.hero")!;
  expect(hero.classList.contains("frame")).toBe(true);
  expect(hero.querySelector("img")?.getAttribute("src")).toBe(PIXEL);
  // Never a letter where the picture would be, never a "Based on" line.
  expect(hero.textContent?.trim()).toBe("");
  expect(screen.container.textContent).not.toContain("Based on");
  // The facts read label over value.
  await expect.element(screen.getByText("Founded", { exact: true })).toBeVisible();
  await expect.element(screen.getByText("2010", { exact: true })).toBeVisible();
  await screen.getByRole("button", { name: "Ask about this", exact: true }).click();
  expect(onask).toHaveBeenCalledWith("Stripe");
  await screen.unmount();
});

test("the table lift sorts by a header, marks the best price and copies CSV", async () => {
  await page.viewport(900, 700);
  const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  const screen = await render(Table, {
    caption: "Plans",
    columns: ["Plan", "Price", "Region"],
    rows: [
      ["Pro", "$1,200", "Oslo"],
      ["Team", "$90", "berlin"],
      ["Starter", "$300", "Amsterdam"],
    ],
  });
  const first = () =>
    [...screen.container.querySelectorAll("tbody tr")].map(
      (row) => row.firstElementChild!.textContent,
    );
  expect(first()).toEqual(["Pro", "Team", "Starter"]);
  // Money sorts as money, not as text: $90 before $300 before $1,200.
  await screen.getByRole("button", { name: "Price" }).click();
  expect(first()).toEqual(["Team", "Starter", "Pro"]);
  await expect
    .element(screen.getByRole("columnheader", { name: "Price" }))
    .toHaveAttribute("aria-sort", "ascending");
  await screen.getByRole("button", { name: "Price" }).click();
  expect(first()).toEqual(["Pro", "Starter", "Team"]);
  // Text sorts by the locale, whatever its case.
  await screen.getByRole("button", { name: "Region" }).click();
  expect(first()).toEqual(["Starter", "Team", "Pro"]);
  // The lowest price carries the card's quiet dot, once.
  const best = screen.container.querySelectorAll(".best");
  expect(best).toHaveLength(1);
  expect(best[0]!.closest("tr")?.firstElementChild?.textContent).toBe("Team");
  await expect.element(screen.getByText("3 rows", { exact: true })).toBeVisible();
  const header = screen.container.querySelector<HTMLElement>("thead th")!;
  expect(getComputedStyle(header).position).toBe("sticky");
  await screen.getByRole("button", { name: "Copy as CSV", exact: true }).click();
  expect(writeText).toHaveBeenCalledWith(
    'Plan,Price,Region\r\nStarter,$300,Amsterdam\r\nTeam,$90,berlin\r\nPro,"$1,200",Oslo',
  );
  writeText.mockRestore();
  await screen.unmount();
});

test("a request card is never under 80 px, with a mark in place of its caption", async () => {
  await page.viewport(900, 700);
  expect(requestSize("Hi").height).toBeGreaterThanOrEqual(80);
  const screen = await render(ObjectiveCard, {
    item: { id: "request", type: "request", title: "Hi", kind: "Request", detail: "", status: "" },
    selected: false,
    onaction: () => {},
  });
  const card = screen.container.querySelector<HTMLElement>(".request")!;
  expect(card.getBoundingClientRect().height).toBeGreaterThanOrEqual(80);
  // The kind is the mark's name, not a caption line.
  await expect.element(screen.getByRole("img", { name: "Request" })).toBeVisible();
  expect(screen.container.textContent?.trim()).toBe("Hi");
  const mark = screen.container.querySelector<HTMLElement>(".mark")!.getBoundingClientRect();
  expect([mark.width, mark.height]).toEqual([16, 16]);
  await screen.unmount();
});

test("findings and cells drawn from what the agent knows carry no chip saying so", async () => {
  await page.viewport(1000, 700);
  const known = { value: { kind: "text", text: "Yes" }, evidence: [], generalKnowledge: true };
  const subjects = [{ name: "Stripe" }];
  const items = [
    {
      claim: "Stripe launched in 2011.",
      subject: 0,
      evidence: [],
      confidence: "inferred" as const,
      generalKnowledge: true,
    },
  ];
  const labels = {
    unknown: "Unknown",
    criterion: "Criterion",
    subject: "Subject",
    yes: "Yes",
    no: "No",
    confidence: {
      supported: "Supported",
      inferred: "Inferred",
      unverified: "Unverified",
      contradicted: "Contradicted",
    },
  };
  const views = [
    await render(FindingsLift, { content: { kind: "findings", subjects, items } }),
    await render(Findings, { subjects, items, labels }),
    await render(Matrix, {
      subjects,
      criteria: [{ name: "Global", kind: "text" }],
      cells: [[known]] as never,
      notes: [],
      labels,
    }),
    await render(Compare, {
      model: compareModel({
        subjects,
        criteria: [{ name: "Global", kind: "text" }],
        cells: [[known]] as never,
        notes: [],
      }),
    }),
  ];
  await expect.element(page.getByText("Stripe launched in 2011.").first()).toBeVisible();
  for (const view of views) {
    expect(view.container.textContent).not.toContain("General knowledge");
    expect(view.container.textContent).not.toContain("general knowledge");
    await view.unmount();
  }
});

test("every lift keeps 48 px from the canvas's edges and its header over a scrolling well", async () => {
  await page.viewport(1000, 760);
  const { default: LiftHost } = await import("./LiftHost.svelte");
  const onclose = vi.fn();
  const screen = await render(LiftHost, { bounds: new DOMRect(0, 0, 1000, 760), onclose });
  const lift = screen.getByRole("dialog", { name: "Stripe" });
  await expect.element(lift).toBeVisible();
  const box = lift.element().getBoundingClientRect();
  expect([box.left, box.top, box.right, box.bottom]).toEqual([48, 48, 952, 712]);
  const well = lift.element().querySelector<HTMLElement>(".content")!;
  const header = () => lift.element().querySelector<HTMLElement>(".lift-header")!;
  expect(header().getBoundingClientRect().top).toBe(box.top);
  well.scrollTop = 600;
  await expect.poll(() => well.hasAttribute("data-lift-scrolled")).toBe(true);
  expect(header().getBoundingClientRect().top).toBe(box.top);
  // The host reads as a quiet link; Escape folds the lift away.
  await expect.element(page.getByRole("button", { name: "stripe.com" })).toBeVisible();
  await userEvent.keyboard("{Escape}");
  await expect.poll(() => onclose.mock.calls.length, { timeout: 2000 }).toBe(1);
  await screen.unmount();
});
