import type { WorkEnvironmentSnapshot, WorkRuntimeProjection } from "$shared/ipc/bindings";
export const projection: WorkRuntimeProjection = {
  version: 1,
  interrupted: [],
  executions: [
    {
      id: "execution",
      approved_revision: "2",
      status: "completed",
      attempts: [],
      spec: {
        plan_revision: "2",
        limits: {
          model_tokens: 100,
          cost_micro_usd: 0,
          operations: 1,
          timeout_seconds: 60,
          max_workers: 1,
        },
        nodes: [
          {
            node: "node",
            parent: null,
            capability: { kind: "synthesize" },
            limits: {
              model_tokens: 100,
              cost_micro_usd: 0,
              operations: 1,
              timeout_seconds: 60,
              max_workers: 1,
            },
          },
        ],
      },
      artifacts: [
        {
          version: 1,
          id: "artifact",
          execution: "execution",
          node: "node",
          attempt: "attempt",
          output: "Findings",
          title: "Dependency findings",
          data: { kind: "document", paragraphs: ["Original evidence"] },
          evidence: [],
          review: "mechanical",
          presentation: "automatic",
        },
      ],
      user_artifacts: [
        {
          artifact: "artifact",
          revision: "4",
          decision: "accepted",
          edited_data: { kind: "document", paragraphs: ["Reviewed findings"] },
          evidence: [],
        },
      ],
    },
  ],
  work: {
    schema_version: 2,
    profile: "profile",
    id: "objective",
    revision: "4",
    lifecycle: "active",
    objective: "Investigate dependencies",
    objective_revision: "1",
    context_revision: "1",
    objective_author: "user",
    questions: [],
    status: "plan_ready",
    plan: null,
  },
};
export const snapshot: WorkEnvironmentSnapshot = {
  version: 1,
  profile: "profile",
  id: "work",
  space: "space",
  title: "Research",
  revision: "1",
  lifecycle: "active",
  areas: [],
  view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
  elements: [
    { id: "objective-card", area: null, reference: { kind: "objective", objective: "objective" } },
    {
      id: "result-card",
      area: null,
      reference: {
        kind: "artifact",
        objective: "objective",
        execution: "execution",
        artifact: "artifact",
      },
    },
  ],
};
