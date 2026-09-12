import type { WorkSurfaceView } from "$features/work";
import type { ArtifactView } from "$shared/ui/data/Artifact";

/** Deterministic presentation fixtures, not a runtime store or contract fixture suite. */
const source = { key: "source-1", label: "Source 1" };
const artifact = (key: string, title: string, content: ArtifactView["content"]): ArtifactView => ({
  key,
  title,
  content,
  evidence: [source],
  reviewLabel: "Source attribution available · human review required",
});
const accommodation = artifact("stay-comparison", "Accommodation shortlist", {
  kind: "comparison",
  criteria: ["Area", "Nightly estimate", "Trade-off"],
  alternatives: [
    { name: "Riverside House", values: ["Old town", "€145", "Walkable; street noise uncertain"] },
    { name: "Garden Studio", values: ["South bank", "€118", "Quiet; longer transit"] },
  ],
});
const transport = artifact("transport-table", "Transport options", {
  kind: "table",
  columns: ["Route", "Duration", "Estimate"],
  rows: [
    ["Direct train", "4 h 20 min", "€62"],
    ["Flight + transfer", "3 h 45 min", "€138"],
  ],
});
const research = artifact("research-note", "Architecture research", {
  kind: "document",
  paragraphs: [
    "The evidence favors a small, explicit boundary between browser resources and execution authority.",
    "A lightweight representation can remain visible while the native resource is released. Review the source before applying this recommendation.",
  ],
});
const chart = artifact("chart", "Response time samples", {
  kind: "chart",
  xLabel: "Sample",
  yLabel: "Milliseconds",
  series: [
    {
      name: "Local baseline",
      points: [
        { label: "First", value: "42.50" },
        { label: "Second", value: "36.20" },
        { label: "Third", value: "38.75" },
      ],
    },
    {
      name: "Candidate",
      points: [
        { label: "First", value: "31.30" },
        { label: "Second", value: "29.10" },
        { label: "Third", value: "30.85" },
      ],
    },
  ],
});
const checklist = artifact("tasks", "Follow-up decisions", {
  kind: "checklist",
  items: [
    { text: "Read the source excerpts", completed: true },
    { text: "Confirm the target platform constraints", completed: false },
    { text: "Review the proposed implementation", completed: false },
  ],
});
const browser = artifact("browser", "Browser resource", {
  kind: "browser",
  title: "SQLite documentation",
  location: "https://www.sqlite.org/docs.html",
  summary:
    "A historical resource reference used by the research worker. Its native context is not retained by this card.",
});
const evidence = artifact("evidence", "Research sources", {
  kind: "sources",
  summary:
    "Historical excerpts support the recommendation. A citation identifies captured evidence, not a live page or a guarantee of correctness.",
});
function item(
  id: string,
  title: string,
  kind: string,
  detail: string,
  status: string,
  artifactKey?: string,
) {
  return { id, title, kind, detail, status, artifactKey };
}
export const scenarios: readonly WorkSurfaceView[] = [
  {
    key: "trip",
    title: "A considered weekend away",
    objective: "Plan a three-day trip with walkable accommodation and a low-stress route.",
    phase: "Clarification required",
    questions: [
      {
        key: "budget",
        prompt: "Which accommodation budget should guide the comparison?",
        options: ["Up to €120 per night", "Up to €160 per night", "Prioritize location"],
      },
    ],
    items: [
      item(
        "plan",
        "Shape the trip",
        "Primary responsibility",
        "Coordinate accommodation and transport research; keep purchase decisions with the user.",
        "Waiting for clarification",
      ),
      item(
        "stays",
        "Find a good place to stay",
        "Accommodation worker",
        "Compare location, nightly cost and uncertainty.",
        "Options ready for review",
        accommodation.key,
      ),
      item(
        "route",
        "Choose the route",
        "Transport worker",
        "Compare door-to-door time and estimated cost.",
        "Options ready for review",
        transport.key,
      ),
    ],
    links: [
      { id: "plan-stays", source: "plan", target: "stays", kind: "dependency" },
      { id: "plan-route", source: "plan", target: "route", kind: "dependency" },
    ],
    artifacts: [accommodation, transport],
    actions: [
      {
        key: "review-purchase",
        label: "Review purchase",
        scope: "Example purchase boundary · exact price, recipient and approval revision required.",
        consequence: "No booking can be made from this preview.",
        disabledReason: "Waiting for an exact runtime-owned approval request",
      },
    ],
  },
  {
    key: "research",
    title: "Choose the browser foundation",
    objective:
      "Compare implementation options, retain evidence and prepare a focused engineering handoff.",
    phase: "Results need review",
    questions: [],
    items: [
      item(
        "primary",
        "Synthesize the findings",
        "Primary responsibility",
        "Keep recommendations separate from observed facts.",
        "Needs review",
        research.key,
      ),
      item(
        "measure",
        "Compare observed timings",
        "Research worker",
        "Inspect sample measurements and exact values.",
        "Samples available",
        chart.key,
      ),
      item(
        "source",
        "Inspect the browser resource",
        "Browser resource",
        "Documentation retained as a descriptive reference.",
        "Historical preview",
        browser.key,
      ),
      item(
        "decisions",
        "Resolve the remaining decisions",
        "Checklist",
        "Review the proposed next steps.",
        "Two decisions remain",
        checklist.key,
      ),
      item(
        "citations",
        "Inspect the evidence",
        "Evidence collection",
        "Read captured source text in context.",
        "Historical evidence",
        evidence.key,
      ),
      item(
        "coding",
        "Prepare the coding handoff",
        "Coding-agent resource",
        "Implementation responsibility and allowed context are assigned by the runtime.",
        "Not connected",
      ),
    ],
    links: [
      { id: "a", source: "primary", target: "measure", kind: "dependency" },
      { id: "b", source: "primary", target: "decisions", kind: "dependency" },
      { id: "c", source: "source", target: "citations", kind: "reference" },
    ],
    artifacts: [research, chart, browser, checklist, evidence],
    actions: [
      {
        key: "accept-result",
        label: "Review result",
        scope: "Research result · demonstration revision 7 · selected artifact only.",
        consequence:
          "This requests a review decision. It does not approve new execution or mark the source as verified.",
      },
    ],
  },
  {
    key: "interrupted",
    title: "Recover an interrupted investigation",
    objective: "Continue the investigation only after reconciling the previous execution.",
    phase: "Outcome unknown",
    notice: {
      title: "The previous worker is no longer connected",
      detail:
        "Historical running facts are retained. They do not prove that a worker is still active. An uncertain effect must be reconciled before retrying.",
    },
    questions: [],
    items: [
      item(
        "owner",
        "Reconcile the previous owner",
        "Primary responsibility",
        "Request current durable facts before deciding what can continue.",
        "Interrupted",
      ),
      item(
        "unknown",
        "Inspect the uncertain effect",
        "Approval boundary",
        "The request may have taken effect before the connection was lost.",
        "Outcome unknown",
      ),
      item(
        "history",
        "Review captured evidence",
        "Evidence collection",
        "Historical sources remain inspectable.",
        "Available",
        evidence.key,
      ),
    ],
    links: [{ id: "recovery", source: "owner", target: "unknown", kind: "dependency" }],
    artifacts: [evidence],
    actions: [
      {
        key: "takeover",
        label: "Request human takeover",
        scope: "Original browser resource and current ownership must be resolved by Rust.",
        consequence: "A preview card does not grant control of a native context.",
        disabledReason: "Ownership reconciliation required",
      },
    ],
  },
];
