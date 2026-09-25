import { SvelteSet } from "svelte/reactivity";
import { commands } from "$shared/ipc/bindings";
import type {
  ResourceCall_Deserialize as ResourceCall,
  ResourceDraft_Deserialize as ResourceDraft,
  ResourceResponse_Serialize as ResourceResponse,
} from "$shared/ipc/bindings";
import type { TaskContext, TaskRow, TaskSession } from "$domain/resources";
import type { ArtifactView, DocumentNodeView, PlanStep } from "$shared/ui/data/Artifact";
import { stepResult } from "./plan-steps";

/** A result's plan as tasks would carry it: one title per step, in order. */
export type StepPlan = {
  /** The result card the steps stand beside. */
  result: string;
  /** The lane's Work, written to the task's reserved `work` field. */
  work: string;
  steps: { title: string; detail: string; context: TaskContext | null }[];
};

export const workTasksKey = Symbol("work-tasks");
/** As `TaskSession.create` bounds a title. */
const TITLE_LIMIT = 256;
const WEB = /^https?:\/\//iu;

const taskTitle = (text: string) => text.replace(/\s+/gu, " ").trim().slice(0, TITLE_LIMIT).trim();
/** The manual position keeps the plan's order: fixed-width keys, as the board writes them. */
const position = (index: number) => String(1_000_000_000 + index * 65_536).padStart(12, "0");
const loose = (text: string) => text.replace(/\s+/gu, " ").trim().toLowerCase();

function plain(node: DocumentNodeView): string {
  if (node.type === "text") return node.text ?? "";
  return (node.content ?? []).map(plain).join(" ");
}
function firstLink(node: DocumentNodeView): string | null {
  for (const mark of node.marks ?? []) {
    const href = mark.type === "link" ? mark.attrs?.href : null;
    if (href && WEB.test(href)) return href;
  }
  for (const child of node.content ?? []) {
    const href = firstLink(child);
    if (href) return href;
  }
  return null;
}
/** The page a written step links to, found by its words in the formatted document. */
function stepLink(view: ArtifactView, text: string): string | null {
  const root = view.content.kind === "document" ? view.content.formatted?.document : null;
  const words = loose(text).slice(0, 40);
  if (!root || !words) return null;
  const blocks: DocumentNodeView[] = [];
  const walk = (node: DocumentNodeView) => {
    if (node.type === "listItem" || node.type === "heading" || node.type === "paragraph")
      blocks.push(node);
    else for (const child of node.content ?? []) walk(child);
  };
  walk(root);
  const block = blocks.find((node) => loose(plain(node)).includes(words));
  return block ? firstLink(block) : null;
}

/** A step's context: the page it links to, else the first page its result cites. */
export function stepPlan(
  result: string,
  work: string,
  view: ArtifactView,
  steps: readonly PlanStep[],
): StepPlan {
  const cited = view.evidence.find((entry) => entry.url && WEB.test(entry.url));
  const fallback = cited?.url ? { url: cited.url, title: cited.label || view.title } : null;
  return {
    result,
    work,
    steps: steps.flatMap((step) => {
      const title = taskTitle(step.text);
      if (!title) return [];
      const href = stepLink(view, step.text);
      const label = view.evidence.find((entry) => entry.url === href)?.label;
      return [
        {
          title,
          detail: (step.detail ?? "").slice(0, 4096),
          context: href ? { url: href, title: (label || title).slice(0, 256) } : fallback,
        },
      ];
    }),
  };
}

/** The steps group's result card, read from its group id. */
export function stepsGroupResult(id: string): string | null {
  const at = id.lastIndexOf(":steps:");
  return id.startsWith("group:") && at >= 0 ? id.slice(at + ":steps:".length) || null : null;
}

/**
 * A plan's steps as the person's tasks. Tasks are made only when asked, one per
 * step, through the resource boundary; what they are now is read from a task
 * session, never kept here.
 */
export class WorkTasks {
  readonly profile: string;
  readonly session: TaskSession;
  #plans: () => ReadonlyMap<string, StepPlan>;
  #making = new SvelteSet<string>();
  #failed = new SvelteSet<string>();
  /** Requests whose outcome was never seen, replayed rather than made twice. */
  #requests = new Map<string, string>();
  #open: (id: string) => void;

  constructor(
    profile: string,
    session: TaskSession,
    plans: () => ReadonlyMap<string, StepPlan>,
    open: (id: string) => void,
  ) {
    this.profile = profile;
    this.session = session;
    this.#plans = plans;
    this.#open = open;
  }

  /** Each step's task, by its title in the lane's Work, in the plan's order. */
  tasks(result: string): (TaskRow | undefined)[] {
    const plan = this.#plans().get(result);
    if (!plan) return [];
    const mine = this.session.rows
      .filter((row) => row.work === plan.work && row.origin === "agent")
      .toSorted((a, b) => (a.sortKey ?? "~").localeCompare(b.sortKey ?? "~"));
    const taken = new Set<string>();
    return plan.steps.map((step) => {
      const row = mine.find((row) => !taken.has(row.id) && row.title === step.title);
      if (row) taken.add(row.id);
      return row;
    });
  }

  /** The task a step card stands for, once there is one. */
  task(step: string): TaskRow | undefined {
    const result = stepResult(step);
    const index = Number(step.slice(step.lastIndexOf(":") + 1));
    return result && Number.isInteger(index) ? this.tasks(result)[index] : undefined;
  }

  /** Whether a result has steps, is making them, or already made every one. */
  state(result: string): "none" | "ready" | "making" | "made" {
    if (!this.#plans().get(result)?.steps.length) return "none";
    if (this.#making.has(result)) return "making";
    const tasks = this.tasks(result);
    return tasks.length && tasks.every(Boolean) ? "made" : "ready";
  }

  get failed(): boolean {
    return this.#failed.size > 0;
  }

  dismiss() {
    this.#failed.clear();
  }

  open(id: string) {
    this.#open(id);
  }

  /** One task per step not yet made; a lane whose steps are all tasks is refused. */
  async make(result: string): Promise<boolean> {
    const plan = this.#plans().get(result);
    if (!plan || this.state(result) !== "ready") return false;
    this.#making.add(result);
    this.#failed.delete(result);
    const existing = this.tasks(result);
    let made = 0;
    let failed = false;
    try {
      for (const index of plan.steps.keys()) {
        if (existing[index]) continue;
        const key = `${plan.work}:${result}:${index}`;
        const request = this.#requests.get(key) ?? crypto.randomUUID();
        this.#requests.set(key, request);
        const response = await this.#call({
          kind: "mutate",
          command: {
            version: 1,
            request_id: request,
            intent: { kind: "create", draft: draft(plan, index) },
          },
        });
        if (response.kind !== "applied" || response.request_id !== request) {
          if (response.kind === "error" && response.error !== "outcome_unknown")
            this.#requests.delete(key);
          failed = true;
          break;
        }
        this.#requests.delete(key);
        void this.#call({ kind: "acknowledge", request_id: request });
        made++;
      }
      // One settlement: the session reads the list again; change events cover the rest.
      if (made) await this.session.reload();
    } finally {
      this.#making.delete(result);
    }
    if (failed) this.#failed.add(result);
    return !failed;
  }

  async #call(call: ResourceCall): Promise<ResourceResponse> {
    try {
      const reply = await commands.resourceCall(this.profile, call);
      return reply.profile === this.profile || reply.response.kind === "error"
        ? reply.response
        : { kind: "error", error: "unavailable" };
    } catch {
      return { kind: "error", error: "outcome_unknown" };
    }
  }
}

/** Drafted as `TaskSession.create` drafts a capture, with the agent's origin and the Work. */
function draft(plan: StepPlan, index: number): ResourceDraft {
  const step = plan.steps[index]!;
  return {
    title: step.title,
    pinned: false,
    related: [],
    content: {
      kind: "task",
      details: {
        list: null,
        inbox: true,
        priority: "none",
        steps: [],
        completed_at: null,
        deadline: null,
        duration: null,
      },
      description: step.detail,
      completed: false,
      status: "open",
      assignee: "user",
      origin: "agent",
      due_date: null,
      due_time: null,
      context: step.context,
      sort_key: position(index),
      work: plan.work,
    },
  };
}
