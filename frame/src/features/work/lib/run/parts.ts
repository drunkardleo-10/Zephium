import type { WorkExecutionFact, WorkPageV1 } from "$shared/ipc/bindings";
import {
  pageGroups,
  sourceRows,
  unreadPages,
  type PageGroup,
  type SourceRow,
  type UnreadPage,
} from "../project-environment-stage";
import { fileName } from "../work-files";
import { hostOf, registrableSite, siteKey, siteName } from "./site";

/** A question waiting on the person about one part: which part (its id, key or site) and the ask card's props. */
export type PartAsk = { part: string; props: Record<string, unknown> };

/** What the agent drew on before it began: memory, a skill, history, notes, tabs, files, a connection, a past work. */
export type RunInputView = {
  kind: "memory" | "skill" | "history" | "notes" | "tabs" | "files" | "connection" | "work";
  label: string;
  count?: number;
  /** Read in this run, or being read now. */
  lit?: boolean;
};

type PartState = "planned" | "running" | "waiting" | "done" | "failed" | "stopped";
type PartHelper = "browser" | "research" | "computer" | "connection";

/**
 * One row of a run: a helper's share of the work. A legacy run has no parts
 * of its own, so its pages are shared out by the site they sit on (Airbnb's
 * `.com` and `.co.uk` are one part), and a run that searched widely has a
 * Search part holding what it cited.
 */
export type RunPart = {
  id: string;
  key: string;
  title: string;
  /** The site whose mark stands for the part. */
  host?: string;
  helper: PartHelper;
  state: PartState;
  pages: PageGroup[];
  unread: UnreadPage[];
  sources: SourceRow[];
  /** Where the part began in its run, for its row's order. */
  order: number;
  /** What the part came to, in its helper's words: "3 homes". */
  summary?: string;
  /** The sites a row of pages that wouldn't open stands for. */
  names?: string[];
  /** The steps a computer part took, in order, for its own view. */
  steps?: string[];
  /** A question waiting on the person about this part. */
  ask?: { props: Record<string, unknown> };
  /** What a computer part touched, one line each: files read and written, commands run. */
  lines?: { kind: "read" | "write" | "command" | "search"; text: string }[];
};

const COMPUTER = new Set([
  "list",
  "read_file",
  "search_files",
  "write_file",
  "edit_file",
  "move_file",
  "delete_file",
  "run_command",
]);

type Step = NonNullable<WorkExecutionFact["steps"]>[number];
type Line = NonNullable<RunPart["lines"]>[number];
/** What a computer step touched, as one line of its part. */
function computerLine(kind: Step["kind"]): Line | null {
  switch (kind.kind) {
    case "run_command":
      return { kind: "command", text: kind.command };
    case "write_file":
    case "edit_file":
    case "delete_file":
      return { kind: "write", text: fileName(kind.path) };
    case "move_file":
      return { kind: "write", text: fileName(kind.to) };
    case "search_files":
      return { kind: "search", text: kind.query };
    case "read_file":
    case "list":
      return { kind: "read", text: fileName(kind.path) };
    default:
      return null;
  }
}
function addLine(part: RunPart, kind: Step["kind"]) {
  const line = computerLine(kind);
  if (line && !part.lines!.some((known) => known.kind === line.kind && known.text === line.text))
    part.lines!.push(line);
}

/**
 * A lead run's parts as its facts name them: each with the pages, sources
 * or files and commands of the steps that carry its id.
 */
function factParts(
  card: string,
  runs: readonly WorkExecutionFact[],
  recorded: readonly WorkPageV1[],
  live: (execution: WorkExecutionFact) => boolean,
): RunPart[] {
  const parts: RunPart[] = [];
  for (const run of runs) {
    const going = live(run);
    for (const fact of run.parts ?? []) {
      const mine = (step: Step) => step.part === fact.id;
      const steps = (run.steps ?? []).filter(mine);
      const ids = new Set(steps.map((step) => step.id));
      const pages = pageGroups(run, recorded).filter((group) =>
        group.steps.some((step) => ids.has(step.id)),
      );
      const unread = unreadPages(run).filter((page) => ids.has(page.key.split(":").at(-1)!));
      const state =
        !going && ["planned", "running", "waiting"].includes(fact.state) ? "stopped" : fact.state;
      const host = fact.service?.host ? registrableSite(fact.service.host) : undefined;
      const part: RunPart = {
        id: `part:${card}:${fact.id}`,
        key: fact.id,
        title: fact.title,
        ...(host ? { host } : {}),
        helper: fact.helper,
        state,
        pages,
        unread,
        sources:
          fact.helper === "research"
            ? sourceRows({ ...run, steps }).filter((row) => !row.file && !!row.url)
            : [],
        order: parts.length,
        ...(fact.summary ? { summary: fact.summary } : {}),
        ...(fact.helper === "computer" || fact.helper === "connection"
          ? { steps: [...ids], lines: [] }
          : {}),
      };
      if (part.lines) for (const step of steps) addLine(part, step.kind);
      parts.push(part);
    }
  }
  return parts;
}

/** A search part appears once search cited this many sites; below it the answer's chips say it. */
const SEARCH_SITES = 3;
const SEARCH_KEY = "search";

type Order = [run: number, step: number];
const before = (a: Order, b: Order) => a[0] - b[0] || a[1] - b[1];

/** The parts of one run, in the order their work began. */
export function runParts(
  card: string,
  runs: readonly WorkExecutionFact[],
  recorded: readonly WorkPageV1[],
  live: (execution: WorkExecutionFact) => boolean,
  titles: { search: string; unread: string; computer: string },
): RunPart[] {
  if (runs.some((run) => run.parts?.length)) return factParts(card, runs, recorded, live);
  const parts = new Map<string, RunPart & { at: Order }>();
  const partFor = (key: string, at: Order, seed: () => Omit<RunPart, "id" | "key" | "order">) => {
    let part = parts.get(key);
    if (!part) {
      part = { id: `part:${card}:${key}`, key, order: 0, at, ...seed() };
      parts.set(key, part);
    } else if (before(at, part.at) < 0) part.at = at;
    return part;
  };
  const stepAt = new Map<string, Order>();
  runs.forEach((run, index) =>
    (run.steps ?? []).forEach((step, at) => stepAt.set(step.id, [index, at])),
  );
  const latestOf = (group: { steps: readonly { id: string }[] }, run: number): Order =>
    group.steps.reduce<Order>(
      (most, step) => {
        const at = stepAt.get(step.id);
        return at && before(at, most) > 0 ? at : most;
      },
      [run, -1],
    );
  /** The part a live run worked in last: it stays open until another takes over. */
  let current: { key: string; at: Order } | null = null;
  const orderOf = (group: { steps: readonly { id: string }[] }, run: number): Order =>
    group.steps.reduce<Order>(
      (least, step) => {
        const at = stepAt.get(step.id);
        return at && before(at, least) < 0 ? at : least;
      },
      [run, Number.MAX_SAFE_INTEGER],
    );
  const browser = () => ({
    helper: "browser" as const,
    state: "done" as const,
    pages: [],
    unread: [],
    sources: [],
    title: "",
  });
  for (const [index, run] of runs.entries()) {
    for (const group of pageGroups(run, recorded)) {
      const host = hostOf(group.url);
      if (!host) continue;
      const part = partFor(siteKey(host), orderOf(group, index), browser);
      part.host ??= registrableSite(host);
      part.pages.push(group);
      if (live(run)) {
        const last = latestOf(group, index);
        if (!current || before(last, current.at) > 0) current = { key: part.key, at: last };
      }
    }
    for (const page of unreadPages(run)) {
      const host = hostOf(page.url);
      if (!host) continue;
      const part = partFor(siteKey(host), [index, Number.MAX_SAFE_INTEGER], browser);
      part.host ??= registrableSite(host);
      part.unread.push(page);
    }
  }
  const cited: SourceRow[] = [];
  const seen = new Set<string>();
  let searched: Order | null = null;
  let searching = false;
  for (const [index, run] of runs.entries()) {
    for (const row of sourceRows(run)) {
      if (row.file || !row.url || seen.has(row.url)) continue;
      seen.add(row.url);
      cited.push(row);
    }
    const steps = run.steps ?? [];
    const first = steps.findIndex((step) => step.kind.kind === "search");
    if (first >= 0) searched ??= [index, first];
    const last = steps.findLastIndex((step) => step.kind.kind === "search");
    if (live(run) && last >= 0 && (!current || before([index, last], current.at) > 0)) {
      current = { key: SEARCH_KEY, at: [index, last] };
      searching = true;
    }
  }
  const sites = new Set(cited.map((row) => siteKey(hostOf(row.url))));
  if (searched && (sites.size >= SEARCH_SITES || (searching && parts.size > 0)))
    partFor(SEARCH_KEY, searched, () => ({
      helper: "research",
      state: "done",
      pages: [],
      unread: [],
      sources: cited,
      title: titles.search,
    }));
  for (const [index, run] of runs.entries()) {
    const steps = run.steps ?? [];
    for (const [at, step] of steps.entries()) {
      const kind = step.kind;
      if (!COMPUTER.has(kind.kind)) continue;
      const part = partFor("computer", [index, at], () => ({
        helper: "computer",
        state: "done",
        pages: [],
        unread: [],
        sources: [],
        title: titles.computer,
        steps: [],
        lines: [],
      }));
      part.steps!.push(step.id);
      addLine(part, kind);
      if (live(run) && (!current || before([index, at], current.at) > 0))
        current = { key: "computer", at: [index, at] };
    }
  }
  const working = current?.key as string | undefined;
  if (working && parts.has(working)) parts.get(working)!.state = "running";
  const list = [...parts.values()].sort((a, b) => before(a.at, b.at));
  for (const part of list)
    if (part.helper === "browser") {
      const titles = part.pages.flatMap((page) =>
        page.steps.flatMap((step) => (step.local?.page_title ? [step.local.page_title] : [])),
      );
      part.title = siteName(part.host ?? part.key, titles);
    }
  // Sites that never opened are one quiet row at the end, not a row each.
  const refused = list.filter(
    (part) => part.helper === "browser" && !part.pages.length && part.state !== "running",
  );
  const kept = list.filter((part) => !refused.includes(part));
  if (refused.length)
    kept.push({
      id: `part:${card}:unread`,
      key: "unread",
      title: titles.unread,
      helper: "browser",
      state: "failed",
      pages: [],
      unread: refused.flatMap((part) => part.unread),
      sources: [],
      order: 0,
      at: [Number.MAX_SAFE_INTEGER, 0],
      names: refused.map((part) => part.title),
    });
  return kept.map(({ at: _at, ...part }, order) => ({ ...part, order }));
}
