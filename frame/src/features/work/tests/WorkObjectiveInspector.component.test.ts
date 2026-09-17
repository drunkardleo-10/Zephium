import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkSession } from "$domain/work";
import WorkObjectiveInspector from "../components/WorkObjectiveInspector.svelte";
import { projection } from "./environment-fixtures";

test("compact objective inspection joins historical plans and keeps exact result attachment and review", async () => {
  const session = new WorkSession("profile");
  session.projection = structuredClone(projection);
  session.selected = "objective";
  const plan = vi.spyOn(session, "plan").mockResolvedValue({
    author: "user",
    revision: "2",
    basis_revision: "1",
    draft: {
      id: "plan",
      nodes: [
        {
          id: "node",
          objective: "Original approved investigation",
          dependencies: [],
          outputs: [
            { name: "Findings", description: "Preserved original output", review: "mechanical" },
          ],
        },
      ],
    },
  });
  const link = { extraction_id: "retained-provider-record", source_id: 2 };
  session.projection!.executions[0]!.user_artifacts![0]!.evidence = [link];
  const readEvidence = vi.spyOn(session, "evidence").mockResolvedValue({
    version: 1,
    link,
    origin: "example.com",
    role: "provider citation",
    text: "Original source text",
    truncated: false,
    source_bytes: "20",
    source: {
      kind: "provider_search",
      provider: "open_ai",
      model: "search-model",
      url: "https://example.com/original",
      title: "Original provider title",
      response_id: "response",
      search_call_id: "search-call",
    },
  });
  const onopencitation = vi.fn();
  const attach = vi.fn();
  const screen = await render(WorkObjectiveInspector, {
    session,
    attached: [],
    onattach: attach,
    onopencitation,
  });
  await expect
    .element(screen.getByText("Original approved investigation", { exact: true }))
    .toBeVisible();
  expect(plan).toHaveBeenCalledExactlyOnceWith("2");
  await expect.element(screen.getByRole("heading", { name: "Request", exact: true })).toBeVisible();
  expect(screen.container.querySelector("h2")?.textContent).not.toContain(
    projection.work.objective,
  );
  await expect
    .element(screen.getByRole("textbox", { name: "Request", exact: true }))
    .toHaveValue(projection.work.objective);
  expect(screen.container.querySelector(".work-canvas")).toBeNull();
  await screen.getByRole("button", { name: "Add result to canvas" }).click();
  expect(attach).toHaveBeenCalledExactlyOnceWith({
    kind: "artifact",
    objective: "objective",
    execution: "execution",
    artifact: "artifact",
  });
  await screen.getByRole("button", { name: "Dependency findings", exact: true }).click();
  await expect.element(screen.getByText("Reviewed findings", { exact: true })).toBeVisible();
  await screen.getByRole("button", { name: "Source 1", exact: true }).click();
  await expect.element(screen.getByText("Original provider title", { exact: true })).toBeVisible();
  expect(readEvidence).toHaveBeenCalledExactlyOnceWith(link);
  expect(onopencitation).not.toHaveBeenCalled();
  await screen.getByRole("button", { name: "Open source in Browse", exact: true }).click();
  expect(onopencitation).toHaveBeenCalledExactlyOnceWith("https://example.com/original");
  // A result reads as finished: no review strip, no "Review required".
  await expect
    .element(screen.getByRole("button", { name: "Edit result", exact: true }))
    .not.toBeInTheDocument();
  expect(screen.container.textContent).not.toContain("Review required");
  await screen.unmount();
  session.dispose();
});

test("exact stale-owner execution review exposes acknowledgment without claiming it is running", async () => {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = { ...structuredClone(projection.executions[0]!), status: "running" as const };
  session.projection = {
    ...structuredClone(projection),
    executions: [execution],
    interrupted: [execution.id],
  };
  vi.spyOn(session, "plan").mockResolvedValue(null);
  const execute = vi.spyOn(session, "execute").mockResolvedValue(false);
  const screen = await render(WorkObjectiveInspector, {
    session,
    attached: [],
    onattach: vi.fn(),
  });
  await expect
    .element(screen.getByRole("button", { name: "Cancel execution", exact: true }))
    .not.toBeInTheDocument();
  expect(screen.container.textContent).not.toContain("running");
  await screen.getByText("Acknowledge interruption", { exact: true }).first().click();
  await expect
    .element(screen.getByRole("button", { name: "Acknowledge interruption", exact: true }))
    .toBeVisible();
  expect(execute).not.toHaveBeenCalled();
  await screen.unmount();
  session.dispose();
});
