import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkSession } from "$domain/work";
import type { WorkCallV1, WorkRuntimeProjection, WorkExecutionSpec } from "$shared/ipc/bindings";
import WorkExecutionReview from "../components/WorkExecutionReview.svelte";

const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call });
});

test.each([false, true])(
  "approves exact native scope and refuses changed basis (provider search: %s)",
  async (providerSearch) => {
    native.call.mockClear();
    const profile = "00000000000000000000000001";
    const work = "00000000000000000000000002";
    const node = "00000000000000000000000003";
    const state: WorkRuntimeProjection = {
      version: 1,
      executions: [],
      interrupted: [],
      owners: [],
      work: {
        id: work,
        profile,
        revision: "9007199254740993",
        schema_version: 2,
        lifecycle: "active",
        objective: "Investigate the issue",
        objective_revision: "1",
        context_revision: "1",
        objective_author: "user",
        questions: [],
        status: "plan_ready",
        plan: {
          author: "primary_agent",
          revision: "2",
          basis_revision: "1",
          draft: {
            id: "00000000000000000000000004",
            nodes: [
              {
                id: node,
                objective: "Investigate public issues",
                dependencies: [],
                outputs: [
                  {
                    name: "Findings",
                    description: "Source-backed findings",
                    review: "source_mapped_needs_review",
                  },
                ],
              },
            ],
          },
        },
      },
    };
    const limits = {
      model_tokens: 10000,
      cost_micro_usd: 100000,
      operations: 20,
      timeout_seconds: 300,
      max_workers: 1,
    };
    const spec: WorkExecutionSpec = {
      plan_revision: "2",
      limits,
      nodes: [
        {
          node,
          parent: null,
          limits,
          capability: providerSearch
            ? {
                kind: "public_search",
                scope: {
                  provider: "open_ai",
                  model: "test-search-model",
                  query: "Svelte pending save unmount",
                },
              }
            : {
                kind: "public_discovery" as const,
                scope: { search_query: "Svelte pending save unmount", max_hops: 8 },
              },
        },
      ],
    };
    const session = new WorkSession(profile);
    session.selected = work;
    session.projection = state;
    session.operations.jobs.set("preparation", {
      id: "preparation",
      input: {
        kind: "prepare_plan",
        request: { version: 1, work, expected_revision: state.work.revision },
      },
      state: {
        kind: "planned",
        response: {
          version: 1,
          profile,
          work,
          basis_revision: state.work.revision,
          usage: null,
          outcome: {
            kind: "settled",
            response: {
              version: 1,
              profile,
              reply: { kind: "approval_draft", work, expected_revision: state.work.revision, spec },
            },
          },
        },
      },
    });
    native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
      expect(call).toMatchObject({
        kind: "execute",
        command: { work, expected_revision: "9007199254740993", intent: { kind: "approve", spec } },
      });
      return { version: 1, profile, reply: { kind: "error", error: "conflict" } };
    });
    const screen = await render(WorkExecutionReview, { session });
    await expect
      .element(screen.getByText("Svelte pending save unmount", { exact: true }))
      .toBeVisible();
    if (providerSearch) {
      await expect
        .element(screen.getByText("Public search via OpenAI · test-search-model", { exact: true }))
        .toBeVisible();
      expect(screen.container.textContent).not.toContain("Bing");
      expect(screen.container.textContent).not.toContain("isolated browser");
    }
    await screen.getByRole("button", { name: "Approve this plan and scope", exact: true }).click();
    await expect.poll(() => session.delivery).toBe("conflict");
    expect(native.call).toHaveBeenCalledTimes(1);
    session.delivery = "ready";
    session.projection = { ...state, work: { ...state.work, revision: "9007199254740994" } };
    await expect
      .element(screen.getByRole("button", { name: "Approve this plan and scope", exact: true }))
      .not.toBeInTheDocument();
    await expect
      .element(screen.getByText("Svelte pending save unmount", { exact: true }))
      .not.toBeInTheDocument();
    await expect
      .element(
        screen.getByText(
          "The Work changed after this proposal. Prepare execution again to review the current plan.",
          { exact: true },
        ),
      )
      .toBeVisible();
    const prepare = vi.spyOn(session.operations, "begin").mockResolvedValue(undefined);
    await screen.getByRole("button", { name: "Prepare execution", exact: true }).click();
    expect(prepare).toHaveBeenCalledExactlyOnceWith({
      kind: "prepare_plan",
      request: {
        version: 1,
        work,
        expected_revision: "9007199254740994",
      },
    });
    expect(native.call).toHaveBeenCalledTimes(1);
    session.projection = {
      ...session.projection!,
      executions: [
        {
          id: "executed",
          approved_revision: "9007199254740994",
          spec,
          status: "interrupted",
          attempts: [],
          artifacts: [],
        },
      ],
    };
    await expect
      .element(
        screen.getByText(
          "The Work changed after this proposal. Prepare execution again to review the current plan.",
          { exact: true },
        ),
      )
      .not.toBeInTheDocument();
    session.dispose();
  },
);
