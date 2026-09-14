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

test.each(["read", "update"] as const)(
  "a signed-in %s approval names origin, page and effect, and requires attestation",
  async (effect) => {
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
        revision: "5",
        schema_version: 2,
        lifecycle: "active",
        objective: "Summarize the current sprint page",
        objective_revision: "1",
        context_revision: "1",
        objective_author: "user",
        questions: [],
        status: "plan_ready",
        plan: {
          author: "user",
          revision: "5",
          basis_revision: "4",
          draft: {
            id: "00000000000000000000000004",
            nodes: [
              {
                id: node,
                objective: "Summarize the current sprint page",
                dependencies: [],
                outputs: [
                  {
                    name: "Findings",
                    description: "What the signed-in page shows, with sources",
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
      model_tokens: 128000,
      cost_micro_usd: 500000,
      operations: 64,
      timeout_seconds: 600,
      max_workers: 1,
    };
    const scope = {
      tab: "tab-1",
      url: "https://app.notion.com/p/Sprint-42",
      origin: "https://app.notion.com",
      account: "01JACCOUNT0000000000000000",
    };
    const spec: WorkExecutionSpec = {
      plan_revision: "5",
      limits,
      nodes: [
        {
          node,
          parent: null,
          limits,
          capability:
            effect === "read"
              ? { kind: "account_read", scope }
              : {
                  kind: "account_update",
                  scope,
                  update: { field: "Title", from: "Sprint 42", to: "Sprint 42 (probe)" },
                },
        },
      ],
    };
    const session = new WorkSession(profile);
    session.selected = work;
    session.projection = state;
    session.operations.jobs.set("account", {
      id: "account",
      input: {
        kind: "prepare_account",
        request: {
          version: 1,
          work,
          expected_revision: "4",
          environment: "00000000000000000000000009",
          element: "00000000000000000000000008",
          effect:
            effect === "read"
              ? { kind: "read" }
              : {
                  kind: "update",
                  update:
                    spec.nodes[0]!.capability.kind === "account_update"
                      ? spec.nodes[0]!.capability.update
                      : { field: null, from: "", to: "" },
                },
        },
      },
      state: {
        kind: "settled",
        response: {
          version: 1,
          profile,
          reply: { kind: "approval_draft", work, expected_revision: "5", spec },
        },
      },
    });
    native.call.mockImplementation((_profile: string, call: WorkCallV1) => {
      expect(call).toMatchObject({
        kind: "execute",
        command: { work, expected_revision: "5", intent: { kind: "approve", spec } },
      });
      return { version: 1, profile, reply: { kind: "error", error: "conflict" } };
    });
    const screen = await render(WorkExecutionReview, { session });
    await expect.element(screen.getByText("https://app.notion.com", { exact: true })).toBeVisible();
    await expect
      .element(screen.getByText("https://app.notion.com/p/Sprint-42", { exact: true }))
      .toBeVisible();
    await expect
      .element(
        screen.getByText(effect === "read" ? "Read only" : "External write, reversible", {
          exact: true,
        }),
      )
      .toBeVisible();
    if (effect === "update")
      await expect
        .element(
          screen.getByText(
            "Change Title from “Sprint 42” to “Sprint 42 (probe)”, then restore it.",
            {
              exact: true,
            },
          ),
        )
        .toBeVisible();
    const approve = screen.getByRole("button", {
      name: "Approve this plan and scope",
      exact: true,
    });
    await expect.element(approve).toBeDisabled();
    await screen.getByRole("checkbox").click();
    await expect.element(approve).toBeEnabled();
    await approve.click();
    await expect.poll(() => session.delivery).toBe("conflict");
    expect(native.call).toHaveBeenCalledTimes(1);
    await screen.unmount();
    session.dispose();
  },
);
