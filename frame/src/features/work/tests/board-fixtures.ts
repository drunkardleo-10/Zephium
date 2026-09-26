import type {
  WorkArtifactDataV1,
  WorkArtifactV1,
  WorkDiagramNodeKind,
  WorkEnvironmentElement,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkPageV1,
  WorkRuntimeProjection,
  WorkStepFact,
} from "$shared/ipc/bindings";
import { organizeExecution } from "../lib/organize";

/** Realistic runs as the agent leaves them, for boards drawn from what they published. */
type Cite = { url: string; title: string };
type Made = {
  id: string;
  title: string;
  data: WorkArtifactDataV1;
  /** Citations this artifact's evidence array names, 1-based. */
  cite?: number[];
  knowledge?: boolean;
};
type Read = { url: string; title?: string; status?: WorkStepFact["status"]; note?: string };

const LIMITS = {
  model_tokens: 100_000,
  cost_micro_usd: 1_000_000,
  operations: 64,
  timeout_seconds: 900,
  max_workers: 4,
};

function run(
  id: string,
  request: string,
  {
    status = "completed",
    citations = [],
    searches = [],
    reads = [],
    made = [],
    said = [],
    extra = [],
  }: {
    status?: WorkExecutionFact["status"];
    citations?: Cite[];
    searches?: string[];
    reads?: Read[];
    made?: Made[];
    said?: string[];
    extra?: WorkStepFact[];
  },
): WorkExecutionFact {
  const search = `${id}-search`;
  const artifact = (entry: Made): WorkArtifactV1 => ({
    version: 1,
    id: entry.id,
    execution: id,
    node: "agent",
    attempt: "attempt",
    output: entry.title,
    title: entry.title,
    data: entry.data,
    evidence: (entry.cite ?? []).map((source_id) => ({ extraction_id: search, source_id })),
    review: "mechanical",
    presentation: "automatic",
    ...(entry.knowledge ? { general_knowledge: true } : {}),
  });
  const collection: Made[] = citations.length
    ? [
        {
          id: `${id}-sources`,
          title: "Sources",
          data: {
            kind: "evidence_collection",
            summary: "",
            entries: citations.map((cite, index) => ({
              evidence: index,
              title: cite.title,
              role: "source",
            })),
          },
          cite: citations.map((_, index) => index + 1),
        },
      ]
    : [];
  const done = status === "completed" || status === "needs_review";
  const steps: WorkStepFact[] = [
    ...said.map((note, index): WorkStepFact => ({
      id: `${id}-turn-${index}`,
      turn: index + 1,
      kind: { kind: "turn" },
      status: "succeeded",
      note,
    })),
    ...searches.map((query, index): WorkStepFact => ({
      id: `${id}-search-${index}`,
      turn: 1,
      kind: { kind: "search", query },
      status: "succeeded",
      ...(index === 0 && citations.length
        ? { artifacts: [`${id}-sources`], evidence: search }
        : {}),
    })),
    ...reads.map((read, index): WorkStepFact => ({
      id: `${id}-read-${index}`,
      turn: 2,
      kind: { kind: "read", url: read.url },
      status: read.status ?? "succeeded",
      ...(read.note ? { note: read.note } : {}),
      measurements: {
        wall_millis: 9_000 + index * 3_500,
        decision_calls: 0,
        emulation_calls: 0,
        planner_calls: 1,
        native_actions: 2,
        model_tokens: 1200,
        cost_micro_usd: 400,
      } as NonNullable<WorkStepFact["measurements"]>,
      ...(read.title
        ? { local: { page_title: read.title } as NonNullable<WorkStepFact["local"]> }
        : {}),
    })),
    ...extra,
    ...(made.length
      ? [
          {
            id: `${id}-publish`,
            turn: 3,
            kind: { kind: "publish" },
            status: done ? "succeeded" : "running",
          } as WorkStepFact,
        ]
      : []),
    ...(done
      ? [
          {
            id: `${id}-finish`,
            turn: 4,
            kind: { kind: "finish" },
            status: "succeeded",
          } as WorkStepFact,
        ]
      : []),
  ];
  return {
    id,
    approved_revision: "1",
    status,
    attempts: [],
    spec: {
      plan_revision: "1",
      request,
      limits: LIMITS,
      nodes: [{ node: "agent", parent: null, capability: { kind: "agent" }, limits: LIMITS }],
    } as WorkExecutionFact["spec"],
    artifacts: [...collection, ...made].map(artifact),
    provider_evidence: citations.length
      ? [
          {
            id: search,
            node: "agent",
            attempt: "attempt",
            evidence: {
              version: 1,
              provider: "open_ai",
              model: "gpt-5.6",
              response_model: "gpt-5.6",
              response_id: "resp",
              search_call_id: "ws",
              answer: "",
              citations: citations.map((cite) => ({ ...cite, start_index: 0, end_index: 1 })),
              actual_input_tokens: 10,
              actual_output_tokens: 10,
            },
          },
        ]
      : [],
    user_artifacts: [],
    steps,
  } as WorkExecutionFact;
}

const PROFILE = "01J8Z7V6Q5N4M3K2H1G0F9E8D7";
/** A picture's digest: the colour its placeholder is drawn in, padded to a digest. */
const picture = (hue: number) => ({
  profile: PROFILE,
  digest: hue.toString(16).padStart(3, "0").repeat(21) + "a",
});

export type BoardScene = {
  name: string;
  snapshot: WorkEnvironmentSnapshot;
  objectives: Map<string, WorkRuntimeProjection>;
  pictures: Map<string, { profile: string; digest: string }>;
  pages: WorkPageV1[];
};

/** A work whose runs placed what organize places, element by element, pictures where a subject has one. */
function scene(
  name: string,
  runs: WorkExecutionFact[],
  pictured: readonly string[] = [],
  framed = true,
): BoardScene {
  const projection: WorkRuntimeProjection = {
    version: 1,
    interrupted: [],
    executions: runs,
    work: {
      schema_version: 2,
      profile: PROFILE,
      id: `${name}-work`,
      revision: "1",
      lifecycle: "active",
      objective: runs[0]!.spec.request ?? name,
      objective_revision: "1",
      context_revision: "1",
      objective_author: "user",
      questions: [],
      status: "plan_ready",
      plan: null,
    },
  } as WorkRuntimeProjection;
  const elements: WorkEnvironmentElement[] = [
    {
      id: `${name}-request`,
      area: null,
      reference: { kind: "objective", objective: projection.work.id },
    },
  ];
  let snapshot: WorkEnvironmentSnapshot = {
    version: 1,
    profile: PROFILE,
    id: `${name}-canvas`,
    space: "space",
    title: name,
    revision: "1",
    lifecycle: "active",
    areas: [],
    view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
    elements,
  } as WorkEnvironmentSnapshot;
  let count = 0;
  for (const execution of runs) {
    const plan = organizeExecution(projection, execution, { x: 0, y: 0 }, snapshot);
    for (const add of plan.adds)
      elements.push({ id: `${name}-${count++}`, area: null, reference: add.reference });
    snapshot = { ...snapshot, elements: [...elements] };
  }
  const pictures = new Map<string, { profile: string; digest: string }>();
  let hue = 17;
  for (const element of elements) {
    const reference = element.reference;
    if (reference.kind !== "subject") continue;
    const execution = runs.find((entry) => entry.id === reference.execution);
    const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
    const subject =
      artifact && "subjects" in artifact.data
        ? artifact.data.subjects?.[reference.index]
        : undefined;
    if (subject && pictured.some((name) => subject.name.includes(name)))
      pictures.set(element.id, picture((hue += 37) % 360));
  }
  const pages: WorkPageV1[] = framed
    ? runs.flatMap((execution) =>
        (execution.steps ?? []).flatMap((step) =>
          step.kind.kind === "read" && step.status === "succeeded"
            ? [
                {
                  execution: execution.id,
                  attempt: "attempt",
                  step: step.id,
                  url: step.kind.url,
                  live: false,
                  frame: { generation: 1, width: 1280, height: 800 },
                },
              ]
            : [],
        ),
      )
    : [];
  return {
    name,
    snapshot,
    objectives: new Map([[projection.work.id, projection]]),
    pictures,
    pages,
  };
}

const ARCHITECTURE_ANSWER = [
  "Start as a modular monolith on managed Postgres behind a CDN, and split out only the parts that scale differently: search, background jobs and file processing.",
  "",
  "The reference architecture below keeps one deployable API for the first year. Tenants share tables with a `tenant_id` on every row and row-level security in Postgres, which is simpler to operate than schema-per-tenant until you pass a few thousand customers.",
  "",
  "## Why this shape",
  "",
  "- **One API, many workers.** Requests stay fast because anything slower than 200 ms goes to the queue.",
  "- **Postgres first.** Full-text search and JSONB cover the first 100k users; add OpenSearch when queries stop fitting in an index.",
  "- **Edge caching.** Static assets and signed file URLs are served by the CDN, so the API never streams bytes.",
  "",
  "## What to decide now",
  "",
  "1. Pick the auth provider before the data model: tenants, roles and invites shape every table.",
  "2. Put billing behind webhooks from day one; retrofitting usage metering is the most expensive change later.",
  "3. Budget for observability early; traces are cheap at launch and priceless at the first outage.",
].join("\n");

/** The SaaS architecture: an answer, 13 parts, a stack table, a cost table and chart, a checklist, a brief. */
export function saasScene(): BoardScene {
  const layer = (id: string, name: string) => ({ id, name });
  const node = (
    id: string,
    name: string,
    kind: WorkDiagramNodeKind,
    layer: string,
    note: string,
    vendor?: string,
  ) => ({ id, name, kind, layer, note, ...(vendor ? { vendor } : {}) });
  const diagram: WorkArtifactDataV1 = {
    kind: "diagram",
    layers: [
      layer("clients", "Clients"),
      layer("edge", "Edge"),
      layer("app", "Application"),
      layer("data", "Data"),
      layer("external", "Services"),
    ],
    nodes: [
      node("web", "Web app", "client", "clients", "Next.js on Vercel", "vercel.com"),
      node("mobile", "Mobile app", "client", "clients", "React Native"),
      node("cdn", "CDN", "edge", "edge", "Static assets, signed URLs", "cloudflare.com"),
      node("gateway", "API gateway", "gateway", "edge", "Auth, rate limits"),
      node("api", "Core API", "service", "app", "Modular monolith, Node"),
      node("jobs", "Job workers", "worker", "app", "Emails, imports, exports"),
      node("search", "Search indexer", "worker", "app", "Syncs to OpenSearch"),
      node("db", "Postgres", "store", "data", "Row-level security", "postgresql.org"),
      node("cache", "Redis", "cache", "data", "Sessions, rate limits", "redis.io"),
      node("queue", "Queue", "queue", "data", "SQS, retries", "aws.amazon.com"),
      node("files", "Object storage", "storage", "data", "S3, per-tenant prefixes"),
      node("auth", "Auth provider", "external", "external", "Clerk", "clerk.com"),
      node("billing", "Billing", "external", "external", "Stripe webhooks", "stripe.com"),
    ],
    edges: [
      { from: "web", to: "cdn", label: "HTTPS" },
      { from: "mobile", to: "gateway", label: "REST" },
      { from: "cdn", to: "gateway", label: "API calls" },
      { from: "gateway", to: "api", label: "JWT" },
      { from: "api", to: "db", label: "SQL" },
      { from: "api", to: "cache", label: "sessions" },
      { from: "api", to: "queue", label: "enqueue" },
      { from: "queue", to: "jobs", label: "consume" },
      { from: "jobs", to: "files", label: "write" },
      { from: "api", to: "search", label: "changes" },
      { from: "gateway", to: "auth", label: "verify" },
      { from: "api", to: "billing", label: "usage" },
      { from: "billing", to: "api", label: "webhooks" },
    ],
  };
  const execution = run(
    "saas",
    "Design a SaaS architecture for a B2B project tool, with stack, costs and a launch checklist",
    {
      said: ["Drafting the architecture from what I know."],
      made: [
        {
          id: "answer",
          title: "How to build a B2B SaaS that scales",
          data: { kind: "answer", markdown: ARCHITECTURE_ANSWER },
          knowledge: true,
        },
        { id: "diagram", title: "Reference architecture", data: diagram, knowledge: true },
        {
          id: "stack",
          title: "Recommended stack",
          knowledge: true,
          data: {
            kind: "table",
            columns: ["Layer", "Technology", "Why"],
            rows: [
              [
                "Frontend",
                "Next.js on Vercel",
                "Server rendering for marketing pages, fast previews for every branch",
              ],
              [
                "API",
                "Node.js with Fastify",
                "One language across the stack, mature ecosystem for SaaS plumbing",
              ],
              [
                "Database",
                "Postgres (Neon)",
                "Row-level security gives tenant isolation without extra services",
              ],
              ["Cache", "Redis (Upstash)", "Sessions and rate limits with pay-per-request pricing"],
              [
                "Queue",
                "AWS SQS",
                "Durable retries and dead-letter queues without running a broker",
              ],
              [
                "Search",
                "OpenSearch",
                "Added once Postgres full-text search stops fitting in memory",
              ],
              ["Auth", "Clerk", "Organizations, invites and SSO out of the box"],
              ["Billing", "Stripe", "Usage metering and invoices through webhooks"],
            ],
          },
        },
        {
          id: "costs",
          title: "Monthly cost by stage",
          knowledge: true,
          data: {
            kind: "table",
            columns: ["Stage", "Infrastructure", "Services", "Total"],
            rows: [
              ["MVP", "$50–150", "$0–100", "$50–250"],
              ["Launch", "$300–800", "$200–500", "$500–1,300"],
              ["Growth", "$2,000–5,000", "$800–2,000", "$2,800–7,000"],
              ["Scale", "$10,000–25,000", "$3,000–8,000", "$13,000–33,000"],
            ],
          },
        },
        {
          id: "chart",
          title: "Monthly cost by stage",
          knowledge: true,
          data: {
            kind: "chart",
            x_label: "Stage",
            y_label: "Monthly cost (USD)",
            series: [
              {
                name: "Total",
                points: [
                  { label: "MVP", value: "$50–250" },
                  { label: "Launch", value: "$500–1,300" },
                  { label: "Growth", value: "$2,800–7,000" },
                  { label: "Scale", value: "$13,000–33,000" },
                ],
              },
            ],
            basis: { method: "Typical published list prices for the stack above" },
            general_knowledge: true,
          },
        },
        {
          id: "checklist",
          title: "Launch checklist",
          knowledge: true,
          data: {
            kind: "checklist",
            items: [
              "Turn on row-level security for every tenant table",
              "Set up Stripe webhooks with signature checks",
              "Add tracing to the API and the job workers",
              "Configure backups with a tested restore",
              "Rate-limit the public API at the gateway",
              "Write the incident runbook",
              "Load-test the three heaviest endpoints",
              "Review the data processing agreement",
            ].map((text) => ({ text, completed: false })),
          },
        },
        {
          id: "brief",
          title: "Architecture brief",
          knowledge: true,
          data: {
            kind: "document",
            paragraphs: [
              "This brief records the decisions behind the reference architecture so the team can revisit them as the product grows.",
              "## Tenancy",
              "Every table carries a tenant_id and Postgres row-level security enforces it. Schema-per-tenant was considered and rejected: migrations would multiply with every customer.",
              "## Background work",
              "Anything that can take longer than 200 ms runs on the queue. Workers are stateless and scale on queue depth.",
              "## Search",
              "Postgres full-text search serves the first year. The indexer is built now so switching to OpenSearch is a configuration change.",
              "## Next steps",
              "1. Choose the auth provider and model organizations.\n2. Build the billing webhooks.\n3. Instrument the API with traces.",
            ],
          },
        },
      ],
    },
  );
  return scene("saas", [execution]);
}

const AIRBNB = (id: string) => `https://www.airbnb.com/rooms/${id}`;
/** The YC trip: stays with pictures, flights, a checklist, cited pages read and one that would not open. */
export function tripScene(): BoardScene {
  const citations: Cite[] = [
    { url: "https://www.ycombinator.com/apply", title: "Apply to Y Combinator" },
    { url: "https://esta.cbp.dhs.gov/", title: "Official ESTA Application Website" },
    { url: AIRBNB("811"), title: "Sunny flat near Caltrain · Airbnb" },
    { url: AIRBNB("452"), title: "Mountain View studio with desk · Airbnb" },
    { url: AIRBNB("937"), title: "Palo Alto guest suite · Airbnb" },
    { url: "https://www.lot.com/us/en/flights", title: "LOT Polish Airlines" },
    { url: "https://www.google.com/travel/flights", title: "Google Flights" },
  ];
  const money = (amount: string) => ({ kind: "money" as const, amount, currency: "USD" });
  const text = (value: string) => ({ kind: "text" as const, text: value });
  const execution = run(
    "trip",
    "Plan my trip from Warsaw to the YC batch in San Francisco: stays near the office, flights and what to do before I go",
    {
      citations,
      searches: [
        "YC winter 2027 batch dates",
        "monthly airbnb mountain view near caltrain",
        "WAW SFO flights january",
      ],
      reads: [
        { url: "https://www.ycombinator.com/apply", title: "Apply to Y Combinator" },
        { url: AIRBNB("811"), title: "Sunny flat near Caltrain" },
        { url: AIRBNB("452"), title: "Mountain View studio with desk" },
        { url: AIRBNB("937"), title: "Palo Alto guest suite" },
        { url: "https://www.lot.com/us/en/flights", title: "LOT Polish Airlines" },
        {
          url: "https://www.kayak.com/flights/WAW-SFO",
          status: "failed",
          note: "The page asked to prove you are human",
        },
      ],
      made: [
        {
          id: "answer",
          title: "Your YC trip, Warsaw to Mountain View",
          cite: [1, 2, 3, 6],
          data: {
            kind: "answer",
            markdown: [
              "Book a monthly stay in Mountain View near Caltrain and fly LOT nonstop from Warsaw; apply for your ESTA at least 72 hours before you fly.",
              "",
              "The batch runs from January 5 to March 20, and the office is a short Caltrain ride from all three stays below. Monthly Airbnb prices drop by roughly a third compared with nightly rates.",
              "",
              "## Getting there",
              "",
              "LOT flies Warsaw to San Francisco nonstop four times a week in January. A connection through Frankfurt is cheaper but adds five hours.",
              "",
              "## Before you go",
              "",
              "Polish citizens travel under the Visa Waiver Program with an approved ESTA, which is valid for two years.",
            ].join("\n"),
          },
        },
        {
          id: "stays",
          title: "Stays near the YC office",
          cite: [3, 4, 5],
          data: {
            kind: "comparison_matrix",
            subjects: [
              {
                name: "Sunny flat near Caltrain",
                descriptor: "Entire apartment in Mountain View",
                homepage: AIRBNB("811"),
              },
              {
                name: "Mountain View studio with desk",
                descriptor: "Entire studio, dedicated workspace",
                homepage: AIRBNB("452"),
              },
              {
                name: "Palo Alto guest suite",
                descriptor: "Private suite with entrance",
                homepage: AIRBNB("937"),
              },
            ],
            criteria: [
              { name: "Price per month", kind: { kind: "text" } },
              { name: "Walk to Caltrain", kind: { kind: "text" } },
              { name: "Rating", kind: { kind: "rating", rubric: "Guest rating", scale_max: 5 } },
              { name: "Workspace", kind: { kind: "presence" } },
            ],
            cells: [
              [
                { value: money("3450"), evidence: [0] },
                { value: text("6 min"), evidence: [0] },
                { value: { kind: "rating", value: 5 } },
                { value: { kind: "presence", present: true } },
              ],
              [
                { value: money("2980"), evidence: [1] },
                { value: text("12 min"), evidence: [1] },
                { value: { kind: "rating", value: 4 } },
                { value: { kind: "presence", present: true } },
              ],
              [
                { value: money("3120"), evidence: [2] },
                { value: text("9 min"), evidence: [2] },
                { value: { kind: "rating", value: 5 } },
                { value: { kind: "presence", present: false } },
              ],
            ],
          },
        },
        {
          id: "flights",
          title: "Flights Warsaw to San Francisco",
          cite: [6, 7],
          data: {
            kind: "comparison_matrix",
            subjects: [
              {
                name: "LOT nonstop WAW → SFO",
                descriptor: "LOT Polish Airlines, 12h 50m",
                homepage: "https://www.lot.com/us/en/flights",
              },
              {
                name: "Lufthansa via Frankfurt",
                descriptor: "1 stop, 17h 40m",
                homepage: "https://www.google.com/travel/flights",
              },
            ],
            criteria: [
              { name: "Fare", kind: { kind: "text" } },
              { name: "Departure", kind: { kind: "text" } },
            ],
            cells: [
              [{ value: money("1180"), evidence: [0] }, { value: text("Jan 3, 10:40") }],
              [{ value: money("812"), evidence: [1] }, { value: text("Jan 3, 06:15") }],
            ],
          },
        },
        {
          id: "facts",
          title: "What to know",
          cite: [1, 2, 3],
          data: {
            kind: "findings",
            subjects: [{ name: "Sunny flat near Caltrain" }],
            items: [
              {
                claim: "The batch starts January 5 and Demo Day is March 20",
                evidence: [0],
                confidence: "supported",
              },
              {
                claim: "An ESTA is valid for two years and takes up to 72 hours",
                evidence: [1],
                confidence: "supported",
              },
              {
                claim: "Monthly stays get a 35% discount",
                subject: 0,
                evidence: [2],
                confidence: "supported",
              },
            ],
          },
        },
        {
          id: "checklist",
          title: "Before you fly",
          cite: [2],
          data: {
            kind: "checklist",
            items: [
              "Apply for ESTA 72 hours before travel",
              "Book the Mountain View stay for Jan 4 – Mar 21",
              "Book LOT nonstop for Jan 3",
              "Buy a Clipper card for Caltrain",
              "Add US travel insurance",
            ].map((text) => ({ text, completed: false })),
          },
        },
      ],
    },
  );
  return scene("trip", [execution], ["Sunny", "Mountain View studio", "Palo Alto"]);
}

/** The job search: eight roles from six sites, each a subject with its salary and place. */
export function jobsScene(): BoardScene {
  const roles: [string, string, string, string, string][] = [
    [
      "Senior Frontend Engineer, Linear",
      "https://linear.app/careers/frontend",
      "$180k–220k",
      "Remote, Europe",
      "Linear",
    ],
    [
      "Product Engineer, Vercel",
      "https://vercel.com/careers/product-engineer",
      "$170k–210k",
      "Remote",
      "Vercel",
    ],
    [
      "Staff Engineer, Figma",
      "https://boards.greenhouse.io/figma/jobs/5123",
      "$240k–290k",
      "San Francisco",
      "Figma",
    ],
    [
      "Frontend Engineer, Raycast",
      "https://jobs.ashbyhq.com/raycast/fe",
      "€110k–140k",
      "Remote, Europe",
      "Raycast",
    ],
    [
      "Senior Engineer, Supabase",
      "https://jobs.ashbyhq.com/supabase/se",
      "$160k–200k",
      "Remote",
      "Supabase",
    ],
    [
      "Design Engineer, Arc",
      "https://jobs.lever.co/thebrowsercompany/de",
      "$190k–230k",
      "New York",
      "Arc",
    ],
    [
      "Web Platform Engineer, Notion",
      "https://boards.greenhouse.io/notion/jobs/6612",
      "$200k–250k",
      "San Francisco",
      "Notion",
    ],
    [
      "Frontend Engineer, Stripe",
      "https://stripe.com/jobs/listing/frontend/5901",
      "$190k–240k",
      "Dublin",
      "Stripe",
    ],
  ];
  const citations = roles.map(([name, url]) => ({ url, title: name }));
  const execution = run(
    "jobs",
    "Find senior frontend roles at product companies I would like, remote or in Europe",
    {
      citations,
      searches: ["senior frontend engineer remote product company", "design engineer jobs europe"],
      reads: roles.slice(0, 6).map(([name, url]) => ({ url, title: name })),
      made: [
        {
          id: "answer",
          title: "Eight roles worth applying to",
          cite: [1, 2, 4],
          data: {
            kind: "answer",
            markdown: [
              "Linear, Raycast and Supabase are the strongest matches: remote in Europe, product-led, and paying in the top band.",
              "",
              "Figma and Notion pay the most but want you in San Francisco; Stripe's Dublin role is the one European office option.",
            ].join("\n"),
          },
        },
        {
          id: "roles",
          title: "Roles",
          cite: roles.map((_, index) => index + 1),
          data: {
            kind: "comparison_matrix",
            subjects: roles.map(([name, url]) => ({
              name,
              homepage: url,
              descriptor: "Full-time",
            })),
            criteria: [
              { name: "Salary", kind: { kind: "text" } },
              { name: "Location", kind: { kind: "text" } },
            ],
            cells: roles.map(([, , salary, location], index) => [
              { value: { kind: "text", text: salary }, evidence: [index] },
              { value: { kind: "text", text: location }, evidence: [index] },
            ]),
          },
        },
      ],
    },
  );
  return scene("jobs", [execution]);
}

/** A Rust ownership explanation: an answer, an annotated example, a small diagram, a table of terms. */
export function rustScene(): BoardScene {
  const execution = run("rust", "Explain how ownership works in Rust with an example", {
    made: [
      {
        id: "answer",
        title: "How Rust ownership works",
        knowledge: true,
        data: {
          kind: "answer",
          markdown: [
            "Every value in Rust has exactly one owner, and the value is freed the moment its owner goes out of scope — no garbage collector needed.",
            "",
            "## Moves",
            "",
            "Assigning a `String` to another variable **moves** it. The old name can no longer be used, which is how Rust prevents two owners from freeing the same memory.",
            "",
            "## Borrowing",
            "",
            "A reference (`&s`) lets code read a value without taking it. You can have many shared borrows or one mutable borrow, never both at once.",
            "",
            "- a borrow never outlives its owner",
            "- the compiler checks this at compile time, so it costs nothing at run time",
          ].join("\n"),
        },
      },
      {
        id: "code",
        title: "Ownership in practice",
        knowledge: true,
        data: {
          kind: "code",
          language: "rust",
          text: [
            "fn main() {",
            '    let s = String::from("hello");',
            "    let t = s; // ownership moves to t",
            '    // println!("{s}"); // error: s was moved',
            "",
            "    let len = measure(&t); // borrow t",
            '    println!("{t} has {len} bytes");',
            "} // t is dropped here, its buffer freed",
            "",
            "fn measure(text: &String) -> usize {",
            "    text.len()",
            "} // the borrow ends; nothing is freed",
          ].join("\n"),
          notes: [
            { from: 3, to: 3, text: "The String moves: s is no longer usable" },
            { from: 6, to: 6, text: "A shared borrow: measure reads t without owning it" },
            { from: 8, to: 8, text: "The owner leaves scope and drop frees the heap buffer" },
          ],
        },
      },
      {
        id: "terms",
        title: "Terms",
        knowledge: true,
        data: {
          kind: "table",
          columns: ["Term", "Meaning"],
          rows: [
            ["Owner", "The variable responsible for freeing a value"],
            ["Move", "Handing ownership to another variable"],
            ["Borrow", "Using a value through a reference without owning it"],
            ["Drop", "Freeing a value when its owner leaves scope"],
            ["Lifetime", "How long a reference is valid"],
          ],
        },
      },
    ],
  });
  return scene("rust", [execution]);
}

/** A dinner request: three places with pictures, their prices and what the pages said. */
export function dinnerScene(): BoardScene {
  const places = [
    [
      "Zuni Café",
      "https://zunicafe.com/",
      "$$$",
      "Mediterranean · Market St",
      "Roast chicken for two, book a week ahead",
    ],
    [
      "Nopa",
      "https://nopasf.com/",
      "$$",
      "Californian · Divisadero",
      "Late kitchen until midnight",
    ],
    ["Kin Khao", "https://kinkhao.com/", "$$", "Thai · Union Square", "One Michelin star"],
  ];
  const citations = places.map(([name, url]) => ({ url: url!, title: name! }));
  const execution = run(
    "dinner",
    "Where should we have dinner in San Francisco on Friday for four?",
    {
      citations,
      searches: ["best dinner san francisco group of four friday"],
      reads: places.map(([name, url]) => ({ url: url!, title: name! })),
      made: [
        {
          id: "answer",
          title: "Dinner for four on Friday",
          cite: [1, 2, 3],
          data: {
            kind: "answer",
            markdown:
              "Zuni Café is the pick for a group of four: the roast chicken is built for sharing and they take tables of four until 9:30. Nopa is the easier booking if Friday fills up.",
          },
        },
        {
          id: "places",
          title: "Places",
          cite: [1, 2, 3],
          data: {
            kind: "comparison_matrix",
            subjects: places.map(([name, url, , descriptor]) => ({
              name: name!,
              homepage: url!,
              descriptor: descriptor!,
            })),
            criteria: [
              { name: "Price", kind: { kind: "text" } },
              { name: "Cuisine", kind: { kind: "text" } },
            ],
            cells: places.map(([, , price, descriptor], index) => [
              { value: { kind: "text", text: price! }, evidence: [index] },
              { value: { kind: "text", text: descriptor!.split(" · ")[0]! }, evidence: [index] },
            ]),
          },
        },
        {
          id: "notes",
          title: "What the pages said",
          cite: [1, 2, 3],
          data: {
            kind: "findings",
            subjects: places.map(([name]) => ({ name: name! })),
            items: places.map(([, , , , claim], index) => ({
              claim: claim!,
              subject: index,
              evidence: [index],
              confidence: "supported" as const,
            })),
          },
        },
      ],
    },
  );
  return scene("dinner", [execution], ["Zuni", "Nopa", "Kin Khao"]);
}

/** The trip while it runs: two pages read, one being read, nothing published yet. */
export function runningScene(): BoardScene {
  const citations: Cite[] = [
    { url: "https://www.ycombinator.com/apply", title: "Apply to Y Combinator" },
    { url: AIRBNB("811"), title: "Sunny flat near Caltrain · Airbnb" },
  ];
  const execution = run("live", "Find a monthly stay near the YC office for January", {
    status: "running",
    citations,
    searches: ["monthly airbnb mountain view near caltrain"],
    reads: [
      { url: "https://www.ycombinator.com/apply", title: "Apply to Y Combinator" },
      { url: AIRBNB("811"), title: "Sunny flat near Caltrain", status: "running" },
    ],
  });
  const built = scene("live", [execution]);
  built.pages = built.pages.map((page) => ({ ...page, live: true }));
  built.pages.push({
    execution: "live",
    attempt: "attempt",
    step: "live-read-1",
    url: AIRBNB("811"),
    live: true,
    frame: { generation: 2, width: 1280, height: 800 },
  });
  return built;
}
