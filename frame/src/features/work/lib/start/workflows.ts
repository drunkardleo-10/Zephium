import * as m from "$shared/i18n/messages";

/** What a workflow's result looks like, drawn small on its tile. Display-only. */
export type WorkflowShape =
  | "flow"
  | "map"
  | "review"
  | "diff"
  | "screens"
  | "board"
  | "landscape"
  | "campaign"
  | "day"
  | "status"
  | "weeks"
  | "exam"
  | "inbox"
  | "people"
  | "market"
  | "bars"
  | "investors";

type WorkflowHue = "sky" | "mint" | "lemon" | "peach" | "rose" | "lilac";

/** The one thing a workflow needs from the person, if any. */
type WorkflowInput = "none" | "text" | "folder";

export type Persona =
  "developer" | "designer" | "marketing" | "manager" | "student" | "office" | "founder";

export type Workflow = {
  /** The built-in skill that does it. */
  skill: string;
  persona: Persona;
  shape: WorkflowShape;
  hue: WorkflowHue;
  input: WorkflowInput;
  /** The places it reads, by the names people know them by. */
  reads: readonly string[];
  title: () => string;
  line: () => string;
  /** The request the composer takes; an input goes at its end. */
  request: () => string;
  /** The one question for its input. */
  ask: () => string;
  /** Words that mark an earlier request as this workflow's. */
  known: RegExp;
};

export const PERSONAS: readonly { id: Persona; label: () => string }[] = [
  { id: "developer", label: m.work_start_developer },
  { id: "designer", label: m.work_start_designer },
  { id: "marketing", label: m.work_start_marketing },
  { id: "manager", label: m.work_start_manager },
  { id: "student", label: m.work_start_student },
  { id: "office", label: m.work_start_office },
  { id: "founder", label: m.work_start_founder },
];

const none = () => "";

export const WORKFLOWS: readonly Workflow[] = [
  {
    skill: "explain-code",
    persona: "developer",
    shape: "flow",
    hue: "lilac",
    input: "folder",
    reads: [],
    title: m.work_flow_explain_code,
    line: m.work_flow_explain_code_line,
    request: m.work_flow_explain_code_request,
    ask: m.work_flow_explain_code_ask,
    known: /\bexplain\b.*\b(code|function|file|module)\b|\bhow does (this|the) code\b/iu,
  },
  {
    skill: "map-a-project",
    persona: "developer",
    shape: "map",
    hue: "sky",
    input: "folder",
    reads: [],
    title: m.work_flow_map_project,
    line: m.work_flow_map_project_line,
    request: m.work_flow_map_project_request,
    ask: m.work_flow_map_project_ask,
    known: /\bmap (the|this|my) (project|repo|codebase)\b|\barchitecture of\b/iu,
  },
  {
    skill: "review-a-change",
    persona: "developer",
    shape: "review",
    hue: "rose",
    input: "folder",
    reads: ["GitHub"],
    title: m.work_flow_review_change,
    line: m.work_flow_review_change_line,
    request: m.work_flow_review_change_request,
    ask: m.work_flow_review_change_ask,
    known: /\breview (the|this|my) (change|pr|pull request|diff|branch)\b/iu,
  },
  {
    skill: "fix-a-bug",
    persona: "developer",
    shape: "diff",
    hue: "mint",
    input: "folder",
    reads: [],
    title: m.work_flow_fix_test,
    line: m.work_flow_fix_test_line,
    request: m.work_flow_fix_test_request,
    ask: m.work_flow_fix_test_ask,
    known: /\bfix (the|a|this) (failing )?(test|bug)s?\b|\btests? (fail|are failing)\b/iu,
  },
  {
    skill: "ui-teardown",
    persona: "designer",
    shape: "screens",
    hue: "rose",
    input: "text",
    reads: [],
    title: m.work_flow_teardown,
    line: m.work_flow_teardown_line,
    request: m.work_flow_teardown_request,
    ask: m.work_flow_teardown_ask,
    known: /\b(tear ?down|teardown)\b/iu,
  },
  {
    skill: "moodboard",
    persona: "designer",
    shape: "board",
    hue: "peach",
    input: "text",
    reads: ["Are.na", "Dribbble"],
    title: m.work_flow_moodboard,
    line: m.work_flow_moodboard_line,
    request: m.work_flow_moodboard_request,
    ask: m.work_flow_moodboard_ask,
    known: /\bmood ?board\b/iu,
  },
  {
    skill: "competitor-landscape",
    persona: "marketing",
    shape: "landscape",
    hue: "sky",
    input: "text",
    reads: [],
    title: m.work_flow_landscape,
    line: m.work_flow_landscape_line,
    request: m.work_flow_landscape_request,
    ask: m.work_flow_landscape_ask,
    known: /\bcompetitors?\b|\bcompetitive landscape\b/iu,
  },
  {
    skill: "campaign-plan",
    persona: "marketing",
    shape: "campaign",
    hue: "peach",
    input: "text",
    reads: [],
    title: m.work_flow_campaign,
    line: m.work_flow_campaign_line,
    request: m.work_flow_campaign_request,
    ask: m.work_flow_campaign_ask,
    known: /\b(launch|marketing) (campaign|plan)\b|\bcampaign plan\b/iu,
  },
  {
    skill: "plan-my-day",
    persona: "manager",
    shape: "day",
    hue: "sky",
    input: "none",
    reads: ["Calendar", "Gmail", "Slack"],
    title: m.work_flow_day,
    line: m.work_flow_day_line,
    request: m.work_flow_day_request,
    ask: none,
    known: /\bplan (my|the) day\b|\bmy day\b/iu,
  },
  {
    skill: "weekly-status",
    persona: "manager",
    shape: "status",
    hue: "lilac",
    input: "none",
    reads: ["Linear", "GitHub", "Slack"],
    title: m.work_flow_status,
    line: m.work_flow_status_line,
    request: m.work_flow_status_request,
    ask: none,
    known: /\b(weekly|week's) (status|update|report)\b|\bstatus update\b/iu,
  },
  {
    skill: "learning-path",
    persona: "student",
    shape: "weeks",
    hue: "lemon",
    input: "text",
    reads: [],
    title: m.work_flow_learn,
    line: m.work_flow_learn_line,
    request: m.work_flow_learn_request,
    ask: m.work_flow_learn_ask,
    known: /\blearn(ing path)?\b|\bstudy plan\b/iu,
  },
  {
    skill: "exam-prep",
    persona: "student",
    shape: "exam",
    hue: "mint",
    input: "text",
    reads: [],
    title: m.work_flow_exam,
    line: m.work_flow_exam_line,
    request: m.work_flow_exam_request,
    ask: m.work_flow_exam_ask,
    known: /\bexam\b|\bfinals?\b|\bmidterm\b/iu,
  },
  {
    skill: "inbox-triage",
    persona: "office",
    shape: "inbox",
    hue: "rose",
    input: "none",
    reads: ["Gmail"],
    title: m.work_flow_inbox,
    line: m.work_flow_inbox_line,
    request: m.work_flow_inbox_request,
    ask: none,
    known: /\b(triage|clean up|go through) (my )?(inbox|mail|email)\b/iu,
  },
  {
    skill: "meeting-prep",
    persona: "office",
    shape: "people",
    hue: "lemon",
    input: "none",
    reads: ["Calendar", "Gmail"],
    title: m.work_flow_meeting,
    line: m.work_flow_meeting_line,
    request: m.work_flow_meeting_request,
    ask: none,
    known: /\bprep(are)?( me)? for (my|the) (next )?meeting\b|\bmeeting prep\b/iu,
  },
  {
    skill: "market-map",
    persona: "founder",
    shape: "market",
    hue: "lilac",
    input: "text",
    reads: [],
    title: m.work_flow_market,
    line: m.work_flow_market_line,
    request: m.work_flow_market_request,
    ask: m.work_flow_market_ask,
    known: /\bmarket map\b|\bmap (the|this) market\b/iu,
  },
  {
    skill: "pricing-research",
    persona: "founder",
    shape: "bars",
    hue: "mint",
    input: "text",
    reads: [],
    title: m.work_flow_pricing,
    line: m.work_flow_pricing_line,
    request: m.work_flow_pricing_request,
    ask: m.work_flow_pricing_ask,
    known: /\bpricing\b|\bhow (do|does) .+ price\b/iu,
  },
  {
    skill: "investor-list",
    persona: "founder",
    shape: "investors",
    hue: "peach",
    input: "text",
    reads: [],
    title: m.work_flow_investors,
    line: m.work_flow_investors_line,
    request: m.work_flow_investors_request,
    ask: m.work_flow_investors_ask,
    known: /\binvestors?\b|\bvcs?\b|\bangels?\b.*\b(raise|round)\b/iu,
  },
];

/** An earlier work as the start screen reads it: its requests and when it was last touched. */
export type RecentWork = { requests: readonly string[]; touched_ms: string };

/** The workflow an earlier request was, by the request it starts with or the words it uses. */
export function workflowOf(request: string): Workflow | null {
  const text = request.trim();
  if (!text) return null;
  const lower = text.toLocaleLowerCase();
  return (
    WORKFLOWS.find((flow) => lower.startsWith(flow.request().trim().toLocaleLowerCase())) ??
    WORKFLOWS.find((flow) => flow.known.test(text)) ??
    null
  );
}

/** Workflows from the person's latest works, most recent first, each once. */
export function recentWorkflows(works: readonly RecentWork[], limit = 3): Workflow[] {
  const found: Workflow[] = [];
  const latest = [...works].sort((a, b) => Number(b.touched_ms) - Number(a.touched_ms));
  for (const work of latest) {
    for (const request of work.requests) {
      const flow = workflowOf(request);
      if (flow && !found.includes(flow)) found.push(flow);
      if (found.length >= limit) return found;
    }
  }
  return found;
}

/** The composer's text for a workflow, with its input when one was chosen. */
export function composed(flow: Workflow, input = ""): string {
  const request = flow.request();
  const value = input.trim();
  if (!value) return request;
  return flow.input === "none" ? `${request}. ${value}` : `${request} ${value}`;
}
