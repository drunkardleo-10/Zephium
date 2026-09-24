import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { WorkSession } from "$domain/work";
import type { WorkExecutionFact, WorkRuntimeProjection } from "$shared/ipc/bindings";
import WorkResultInspector from "../components/WorkResultInspector.svelte";
import WorkObjectiveInspector from "../components/WorkObjectiveInspector.svelte";
import { projection as base } from "./environment-fixtures";

const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call });
});

type Step = NonNullable<WorkExecutionFact["steps"]>[number];
const citations = ["a.example", "b.example", "c.example"].map((host, index) => ({
  url: `https://${host}/page`,
  title: `Page ${index + 1}`,
  start_index: 0,
  end_index: 1,
}));

function state(data: WorkExecutionFact["artifacts"][number]["data"], steps: Step[] = []) {
  const projection: WorkRuntimeProjection = structuredClone(base);
  const execution = projection.executions[0]!;
  execution.user_artifacts = [];
  execution.provider_evidence = [
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
        citations,
      },
    },
  ];
  execution.artifacts = [
    {
      ...execution.artifacts[0]!,
      title: "What we found",
      data,
      evidence: citations.map((_, index) => ({ extraction_id: "record", source_id: index + 1 })),
    },
  ];
  execution.steps = steps;
  const session = new WorkSession("profile");
  session.selected = "objective";
  session.projection = projection;
  return session;
}
const reference = {
  kind: "artifact" as const,
  objective: "objective",
  execution: "execution",
  artifact: "artifact",
};

test("a result lifts in reading mode with one collapsed sources rail and no chips in the text", async () => {
  await page.viewport(900, 700);
  const session = state({ kind: "document", paragraphs: ["The lead.", "The second paragraph."] });
  const onopen = vi.fn();
  const screen = await render(WorkResultInspector, { session, reference, source: null, onopen });
  await expect.element(screen.getByRole("heading", { name: "What we found" })).toBeVisible();
  await expect.element(screen.getByText("The second paragraph.")).toBeVisible();
  expect(screen.container.querySelector(".reading")).not.toBeNull();
  expect(screen.container.querySelector(".chip")).toBeNull();
  const rail = screen.getByRole("button", { name: "Based on 3 sources" });
  await expect.element(rail).toHaveAttribute("aria-expanded", "false");
  expect(screen.container.querySelectorAll(".rail li")).toHaveLength(0);
  await rail.click();
  expect(screen.container.querySelectorAll(".rail li")).toHaveLength(3);
  await screen.getByRole("button", { name: /b\.example/ }).click();
  expect(onopen).toHaveBeenCalledExactlyOnceWith("https://b.example/page");
  await screen.unmount();
  session.dispose();
});

test("a findings card lifts with every claim, its detail and its evidence", async () => {
  await page.viewport(900, 700);
  const claims = ["One", "Two", "Three", "Four", "Five", "Six"].map((word, index) => ({
    claim: `Claim ${word}`,
    detail: `Detail ${word}`,
    confidence: "supported" as const,
    evidence: [index % 3],
  }));
  const session = state({ kind: "findings", subjects: [], items: claims });
  const screen = await render(WorkResultInspector, { session, reference, source: null });
  for (const { claim, detail } of claims) {
    await expect.element(screen.getByText(claim, { exact: true })).toBeVisible();
    await expect.element(screen.getByText(detail, { exact: true })).toBeVisible();
  }
  await expect.element(screen.getByText("6 claims")).toBeVisible();
  expect(screen.container.textContent).not.toContain("+2");
  await screen.unmount();
  session.dispose();
});

test("a request lifts whole with its run as a quiet timeline", async () => {
  await page.viewport(900, 700);
  const step = (id: string, kind: Step["kind"], status: Step["status"], extra = {}) =>
    ({ id, turn: 1, kind, status, ...extra }) as Step;
  const session = state({ kind: "document", paragraphs: ["Done."] }, [
    step("s1", { kind: "turn" }, "succeeded", { note: "Looking for reviews first." }),
    step("s2", { kind: "search", query: "quiet keyboards" }, "succeeded"),
    step("s3", { kind: "read", url: "https://www.a.example/review" }, "succeeded", {
      measurements: {
        wall_millis: 3200,
        decision_calls: 0,
        emulation_calls: 0,
        planner_calls: 0,
        native_actions: 0,
        model_tokens: 0,
        cost_micro_usd: 0,
        cost_basis: "exact",
      },
    }),
    step("s4", { kind: "read", url: "https://b.example/blocked" }, "failed", {
      note: "The site asked for a sign-in.",
    }),
    step("s5", { kind: "finish" }, "succeeded"),
  ]);
  session.projection!.executions[0]!.spec.nodes[0]!.capability = {
    kind: "agent",
    grant: { provider: "open_ai", model: "m", max_turns: 4, max_steps: 8, browse_hops: 2 },
  };
  vi.spyOn(session, "plan").mockResolvedValue(null);
  const screen = await render(WorkObjectiveInspector, {
    session,
    attached: [],
    onattach: vi.fn(),
  });
  await expect
    .element(screen.getByRole("heading", { name: "Investigate dependencies" }))
    .toBeVisible();
  const timeline = screen.getByRole("list", { name: "What the agent did" });
  await expect.element(timeline).toBeVisible();
  const rows = [...screen.container.querySelectorAll(".timeline li")].map((row) =>
    [...row.querySelectorAll(":scope > :not(.glyph)")]
      .map((part) => part.textContent?.trim())
      .join(" "),
  );
  expect(rows).toEqual([
    "Looking for reviews first.",
    "Searched quiet keyboards",
    "Read a.example 3.2 s",
    "Read b.example The site asked for a sign-in.",
    "Finished",
  ]);
  expect(screen.container.textContent).not.toContain("s3");
  // An agent run needs no plan: the plan controls fold away.
  expect(screen.container.querySelector("details.planning")?.hasAttribute("open")).toBe(false);
  await screen.unmount();
  session.dispose();
});
