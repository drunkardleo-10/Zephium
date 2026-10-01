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

/** A trip plan as a run writes it: a lead, sections, and four next steps. */
const TRIP_PLAN = [
  "# Poland → San Francisco for a YC batch",
  "Planning note: dates and budget weren’t provided, so this plan keeps every booking open.",
  "## YC timing",
  "The on-time deadline is November 2 at 8 p.m. PT.",
  "## Entry and arrival",
  "Polish citizens may use the Visa Waiver Program with an approved ESTA.",
  "## Suggested next steps",
  [
    "1. Confirm the target YC batch and its current application/interview schedule; use that to choose tentative travel dates.",
    "2. Check your ESTA eligibility and official entry requirements. [3, 5]",
    "3. Once dates and a budget are set, compare Airbnb flats for total cost.",
    "4. Plan the SFO-to-stay route by BART.",
  ].join("\n"),
];

/** An architecture-style run: a request and one document, no sources, pages or subjects. */
export function planScene(): {
  scene: WorkEnvironmentSnapshot;
  objectives: Map<string, WorkRuntimeProjection>;
} {
  const state = structuredClone(projection);
  const run = state.executions[0]!;
  run.user_artifacts = [];
  run.artifacts = [
    {
      ...run.artifacts[0]!,
      id: "plan",
      title: "Winter 2027 YC trip plan",
      data: { kind: "document", paragraphs: TRIP_PLAN },
    },
  ];
  return {
    scene: {
      ...snapshot,
      elements: [
        snapshot.elements[0]!,
        {
          id: "plan-card",
          area: null,
          reference: {
            kind: "artifact",
            objective: "objective",
            execution: "execution",
            artifact: "plan",
          },
        },
      ],
      relations: [
        // The person asked about the result: the goal uses it. No edge is drawn for that.
        {
          id: "asked",
          from: "objective-card",
          to: "plan-card",
          kind: "uses",
          origin: { kind: "user" },
        },
      ],
      view: {
        ...snapshot.view,
        placements: [{ element: "objective-card", x: 0, y: 0, width: 300, height: 110 }],
      },
    } as WorkEnvironmentSnapshot,
    objectives: new Map([["objective", state]]),
  };
}
