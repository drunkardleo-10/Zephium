import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page, userEvent } from "vitest/browser";
import { WorkSession } from "$domain/work";
import type { WorkCallV1, WorkRuntimeProjection } from "$shared/ipc/bindings";
import WorkResultInspector from "../components/WorkResultInspector.svelte";
import WorkSubjectInspector from "../components/WorkSubjectInspector.svelte";

const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call });
});

const profile = "00000000000000000000000001";
const work = "00000000000000000000000002";

function projection(): WorkRuntimeProjection {
  const limits = {
    model_tokens: 10,
    cost_micro_usd: 0,
    operations: 1,
    timeout_seconds: 60,
    max_workers: 1,
  };
  return {
    version: 1,
    interrupted: [],
    executions: [
      {
        id: "execution",
        approved_revision: "2",
        status: "needs_review",
        attempts: [],
        spec: { plan_revision: "2", limits, nodes: [] },
        provider_evidence: [
          {
            id: "record",
            node: "node",
            attempt: "attempt",
            evidence: {
              version: 1,
              provider: "open_ai",
              model: "search-model",
              response_model: "search-model",
              response_id: "response",
              search_call_id: "search-call",
              answer: "",
              actual_input_tokens: 0,
              actual_output_tokens: 0,
              citations: [
                {
                  url: "https://lego.com/tower-bridge",
                  title: "Tower Bridge",
                  start_index: 0,
                  end_index: 1,
                },
              ],
            },
          },
        ],
        artifacts: [
          {
            version: 1,
            id: "artifact",
            execution: "execution",
            node: "node",
            attempt: "attempt",
            output: "Comparison",
            title: "Sets compared",
            review: "source_mapped_needs_review",
            presentation: "automatic",
            evidence: [{ extraction_id: "record", source_id: 1 }],
            data: {
              kind: "comparison_matrix",
              subjects: [
                { name: "Tower Bridge", homepage: "https://lego.com/tower-bridge" },
                { name: "Eiffel Tower" },
              ],
              criteria: [
                { name: "Price", kind: { kind: "text" } },
                { name: "Minifigures", kind: { kind: "text" } },
              ],
              cells: [
                [
                  { value: { kind: "money", amount: "239.99", currency: "USD" }, evidence: [0] },
                  { value: { kind: "text", text: "Yes" }, evidence: [0] },
                ],
                [
                  { value: { kind: "money", amount: "629.99", currency: "USD" }, evidence: [0] },
                  { value: { kind: "text", text: "No" }, evidence: [] },
                ],
              ],
              notes: [],
            },
          },
        ],
      },
    ],
    work: {
      schema_version: 2,
      profile,
      id: work,
      revision: "4",
      lifecycle: "active",
      objective: "Compare two sets",
      objective_revision: "1",
      context_revision: "1",
      objective_author: "user",
      questions: [],
      status: "plan_ready",
      plan: null,
    },
  };
}

test("a comparison reads as a product compare, a cell is corrected in place, and the result is accepted once", async () => {
  await page.viewport(1200, 800);
  const session = new WorkSession(profile);
  session.selected = work;
  session.projection = projection();
  const intents: unknown[] = [];
  native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
    if (call.kind === "execute") intents.push(call.command.intent);
    return { version: 1, profile, reply: { kind: "error", error: "conflict" } };
  });
  const onopen = vi.fn();
  const screen = await render(WorkResultInspector, {
    session,
    reference: { kind: "artifact", objective: work, execution: "execution", artifact: "artifact" },
    source: null,
    pictures: new Map(),
    onopen,
  });
  // Price leads each column; it is not one of the rows.
  await expect.element(screen.getByText("$239.99", { exact: true })).toBeVisible();
  await expect.element(screen.getByRole("rowheader", { name: "Minifigures" })).toBeVisible();
  expect(screen.container.querySelector("th.criterion")?.textContent).not.toContain("Price");
  // Yes/no reads as glyphs, and the cell's source opens in the pane.
  expect(screen.container.querySelectorAll('[aria-label="Yes"], [aria-label="No"]').length).toBe(2);
  await screen.getByRole("button", { name: "lego.com", exact: true }).first().click();
  expect(onopen).toHaveBeenCalledWith("https://lego.com/tower-bridge");
  // Correcting one cell commits through the edit path and leaves the rest alone.
  await screen.getByRole("button", { name: "Correct", exact: true }).first().click();
  const editor = screen.getByRole("textbox", { name: /Minifigures/ }).first();
  await editor.fill("No");
  await userEvent.keyboard("{Enter}");
  await expect
    .poll(() => intents)
    .toEqual([
      {
        kind: "edit_artifact",
        execution: "execution",
        artifact: "artifact",
        evidence: [{ extraction_id: "record", source_id: 1 }],
        data: expect.objectContaining({ kind: "comparison_matrix" }),
      },
    ]);
  const edited = (intents[0] as { data: { cells: { value: unknown }[][] } }).data;
  expect(edited.cells[0]?.[1]?.value).toEqual({ kind: "text", text: "No" });
  expect(edited.cells[1]?.[0]?.value).toEqual({
    kind: "money",
    amount: "629.99",
    currency: "USD",
  });
  session.discardArtifact("artifact");
  // One small Accept, because Rust keeps the run in review until it is given.
  await screen.getByRole("button", { name: "Accept", exact: true }).click();
  await expect.poll(() => intents.length).toBe(2);
  expect(intents[1]).toEqual({
    kind: "review_artifact",
    execution: "execution",
    artifact: "artifact",
    decision: "accepted",
  });
  await screen.unmount();
  session.dispose();
});

test("a subject opens as a product view with its facts, their sources, and its page", async () => {
  await page.viewport(1200, 800);
  const onopen = vi.fn();
  const screen = await render(WorkSubjectInspector, {
    reference: {
      kind: "subject",
      objective: work,
      execution: "execution",
      artifact: "artifact",
      index: 0,
    },
    objectives: new Map([[work, projection()]]),
    pictures: [],
    onopen,
  });
  await expect.element(screen.getByRole("heading", { name: "Tower Bridge" })).toBeVisible();
  await expect.element(screen.getByText("$239.99", { exact: true })).toBeVisible();
  await expect.element(screen.getByText("Minifigures", { exact: true })).toBeVisible();
  await screen.getByRole("button", { name: "Open page", exact: true }).click();
  expect(onopen).toHaveBeenCalledWith("https://lego.com/tower-bridge");
  // The page behind the fact is listed once, open, and opens in the pane; the list is the statement.
  const sources = screen.getByRole("complementary", { name: "Sources" });
  await expect.element(sources.getByRole("listitem")).toHaveLength(1);
  expect(screen.container.textContent).not.toContain("Based on");
  await sources.getByRole("listitem").getByRole("button").click();
  expect(onopen).toHaveBeenLastCalledWith("https://lego.com/tower-bridge");
  await screen.unmount();
});

test("the open compare keeps its header and first column in view and marks the best value", async () => {
  await page.viewport(1200, 800);
  const { default: Compare } = await import("../components/compare/Compare.svelte");
  const { compareModel } = await import("../lib/compare");
  const cell = (value: object) => ({ value, evidence: [], generalKnowledge: false });
  const model = compareModel({
    subjects: [{ name: "Tower Bridge" }, { name: "Eiffel Tower" }],
    criteria: [
      { name: "Price", kind: "text" },
      { name: "Rating", kind: "rating", scaleMax: 5 },
      { name: "Minifigures", kind: "presence" },
    ],
    cells: [
      [
        cell({ kind: "money", amount: "239.99", currency: "USD" }),
        cell({ kind: "rating", value: 4.2 }),
        cell({ kind: "presence", present: true }),
      ],
      [
        cell({ kind: "money", amount: "629.99", currency: "USD" }),
        cell({ kind: "rating", value: 4.8 }),
        cell({ kind: "presence", present: false }),
      ],
    ] as never,
    notes: [],
  });
  const screen = await render(Compare, { model });
  const corner = screen.container.querySelector<HTMLElement>("th.corner")!;
  expect(getComputedStyle(corner).position).toBe("sticky");
  expect(getComputedStyle(corner).insetInlineStart).toBe("0px");
  const criterion = screen.container.querySelector<HTMLElement>("th.criterion")!;
  expect(getComputedStyle(criterion).position).toBe("sticky");
  const picture = screen.container.querySelector<HTMLElement>(".picture")!;
  expect(picture.getBoundingClientRect().width).toBe(40);
  // The lower price and the higher rating each carry one quiet dot.
  expect(screen.container.querySelectorAll(".best")).toHaveLength(2);
  const check = screen.container.querySelector<HTMLElement>("td.check .cell")!;
  expect(getComputedStyle(check).textAlign).toBe("center");
  await expect.element(screen.getByLabelText("Yes")).toBeVisible();
  await expect.element(screen.getByLabelText("No")).toBeVisible();
  await screen.unmount();
});
