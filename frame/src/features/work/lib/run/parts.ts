import type { WorkExecutionFact, WorkPageV1, WorkPartNeedV1 } from "$shared/ipc/bindings";
import {
  pageGroups,
  sourceRows,
  unreadPages,
  type PageGroup,
  type SourceRow,
  type UnreadPage,
} from "../project-environment-stage";
import { fileName } from "../work-files";
import { leadRun } from "../agent-steps";
import * as m from "$shared/i18n/messages";
import { runSources } from "./sources";
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
  /** A folder the run read, by its own name. */
  folder?: boolean;
  /** The canvas element it stands for, opened from its mark. */
  element?: string;
};

type PartState = "planned" | "running" | "waiting" | "done" | "failed" | "stopped";
type PartHelper = "browser" | "research" | "computer" | "connection" | "lead";

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
  /** The connection a connection part works through: "github". */
  connection?: string;
  /** What the part came to, in its helper's words: "3 homes". */
  summary?: string;
  /** What the lead gave it to do: "Warsaw to San Francisco and back". */
  goal?: string;
  /** The sites a row of pages that wouldn't open stands for. */
  names?: string[];
  /** The steps a computer part took, in order, for its own view. */
  steps?: string[];
  /** A question waiting on the person about this part. */
  ask?: { props: Record<string, unknown> };
  /** What the part could not do without the person, with the thing it concerns. */
  need?: WorkPartNeedV1;
  /** What a computer part touched, one line each: files read and written, commands run. */
  lines?: { kind: "read" | "write" | "command" | "search"; text: string }[];
  /** While it works, what it is doing now, in words: "Reading Lux 2br near YC". */
  now?: string;
};

/** What a step is doing while it runs, as a live part's row says it. */
function nowWords(step: Step): string | undefined {
  const kind = step.kind;
  switch (kind.kind) {
    case "read":
      return m.work_line_reading({ host: (hostOf(kind.url) || kind.url).replace(/^www\./u, "") });
    case "search":
    case "discover":
      return m.work_now_searching({ query: kind.query });
    case "list":
      return m.work_line_listing_files({ name: fileName(kind.path) });
    case "read_file":
      return m.work_line_reading_file({ name: fileName(kind.path) });
    case "search_files":
      return m.work_now_searching({ query: kind.query });
    case "run_command":
      return m.work_now_running({ command: kind.command });
    case "write_file":
    case "edit_file":
      return m.work_line_writing_file({ name: fileName(kind.path) });
    case "ask":
      return m.work_line_waiting_for_you();
    default:
      return step.note?.trim() || undefined;
  }
}
/** A live part's newest running step, in words. */
const nowOf = (steps: readonly Step[]) =>
  steps
    .filter((step) => step.status === "running")
    .map(nowWords)
    .findLast((words) => !!words);

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

/** A step a connection part draws as a row: a call (a read, before calls were their own) or a confirmation. */
const callRow = (step: Step) =>
  step.kind.kind === "confirm" ||
  ((step.kind.kind === "call" || step.kind.kind === "read") && !!step.note);

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
            ? runSources([{ ...run, steps, artifacts: [] }]).rows.flatMap((row) =>
                row.url ? [{ key: row.key, url: row.url, where: row.host, title: row.title }] : [],
              )
            : [],
        order: parts.length,
        ...(fact.summary ? { summary: fact.summary } : {}),
        ...(fact.goal?.trim() ? { goal: fact.goal.trim() } : {}),
        ...(going && state === "running" && nowOf(steps) ? { now: nowOf(steps)! } : {}),
        ...(fact.need && !going ? { need: fact.need } : {}),
        ...(fact.service?.connection ? { connection: fact.service.connection } : {}),
        ...(fact.helper === "computer"
          ? { steps: [...ids], lines: [] }
          : fact.helper === "connection"
            ? // Its rows: each call, and each held confirmation.
              { steps: steps.filter(callRow).map((step) => step.id), lines: [] }
            : {}),
      };
      if (part.lines) for (const step of steps) addLine(part, step.kind);
      parts.push(part);
    }
    const lead = leadReads(card, run, recorded, going);
    if (lead) parts.push({ ...lead, order: parts.length });
  }
  return parts;
}

/**
 * The pages the lead read itself, outside any part, as a row of their own
 * under its mark: named by their sites, live while one is being read.
 */
function leadReads(
  card: string,
  run: WorkExecutionFact,
  recorded: readonly WorkPageV1[],
  going: boolean,
): RunPart | null {
  const steps = (run.steps ?? []).filter((step) => !step.part && step.kind.kind === "read");
  if (!steps.length) return null;
  const ids = new Set(steps.map((step) => step.id));
  const pages = pageGroups(run, recorded).filter((group) =>
    group.steps.some((step) => ids.has(step.id)),
  );
  const unread = unreadPages(run).filter((page) => ids.has(page.key.split(":").at(-1)!));
  if (!pages.length && !unread.length) return null;
  const sites = [
    ...new Set(
      [...pages.map((page) => page.url), ...unread.map((page) => page.url)].map((url) =>
        registrableSite(hostOf(url)),
      ),
    ),
  ].filter(Boolean);
  const titled = (site: string) =>
    pages.flatMap((page) =>
      registrableSite(hostOf(page.url)) === site
        ? [
            ...page.steps.flatMap((step) =>
              step.local?.page_title ? [step.local.page_title] : [],
            ),
            ...(page.page?.title ? [page.page.title] : []),
          ]
        : [],
    );
  const names = sites.map((site) => siteName(site, titled(site)));
  const reading = going && steps.some((step) => step.status === "running");
  return {
    id: `part:${card}:lead:${run.id}`,
    key: `lead:${run.id}`,
    title:
      names.length > 2
        ? m.work_part_lead_sites({ first: names[0]!, second: names[1]!, count: names.length - 2 })
        : names.join(" · "),
    ...(sites.length === 1 ? { host: sites[0]! } : {}),
    helper: "lead",
    state: reading ? "running" : "done",
    pages,
    unread,
    sources: [],
    order: 0,
    ...(reading && nowOf(steps) ? { now: nowOf(steps)! } : {}),
  };
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
  // A lead run's parts are the ones it recorded, none when it did the work itself.
  if (runs.some((run) => run.parts?.length || leadRun(run)))
    return factParts(card, runs, recorded, live);
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
  if (working && parts.has(working)) {
    const part = parts.get(working)!;
    part.state = "running";
    const steps = runs.flatMap((run) => (live(run) ? (run.steps ?? []) : []));
    const mine = new Set([
      ...part.pages.flatMap((page) => page.steps.map((step) => step.id)),
      ...(part.steps ?? []),
    ]);
    const now = nowOf(
      working === SEARCH_KEY
        ? steps.filter((step) => step.kind.kind === "search")
        : steps.filter((step) => mine.has(step.id)),
    );
    if (now) part.now = now;
  }
  const list = [...parts.values()].sort((a, b) => before(a.at, b.at));
  for (const part of list)
    if (part.helper === "browser") {
      // The site's own name, as its pages and what search cited from it title themselves.
      const titles = [
        ...part.pages.flatMap((page) =>
          page.steps.flatMap((step) => (step.local?.page_title ? [step.local.page_title] : [])),
        ),
        ...cited.flatMap((row) => (siteKey(hostOf(row.url)) === part.key ? [row.title] : [])),
      ];
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

/** One thing the lead did on this Mac itself, as a quiet line under the request. */
export type LocalRead = { kind: "folder" | "file" | "search" | "command" | "change"; text: string };

/** The files a folder listing found: its lines that carry a size, not its folders. */
function listedFiles(run: WorkExecutionFact, step: Step): number {
  const record = run.file_evidence?.find((entry) => entry.id === step.evidence);
  const text = record?.file.text;
  if (!text) return 0;
  return text.split("\n").filter((line) => line.includes("\t")).length;
}

/**
 * What the lead read or ran on this Mac itself, outside any part, as quiet
 * lines under the request: "Read Lunios · 38 files", "Read package.json",
 * "Ran 2 commands". A run with parts keeps their work on their rows.
 */
export function localReads(runs: readonly WorkExecutionFact[]): LocalRead[] {
  const listed = new Map<string, number>();
  const read: string[] = [];
  const searched: string[] = [];
  const ran: string[] = [];
  const changed: string[] = [];
  const once = (list: string[], value: string) => void (list.includes(value) || list.push(value));
  for (const run of runs) {
    if (!leadRun(run)) continue;
    for (const step of run.steps ?? []) {
      if (step.part || step.status === "failed" || step.status === "cancelled") continue;
      const kind = step.kind;
      switch (kind.kind) {
        case "list": {
          listed.set(kind.path, Math.max(listed.get(kind.path) ?? 0, listedFiles(run, step)));
          break;
        }
        case "read_file":
          once(read, fileName(kind.path));
          break;
        case "search_files":
          once(searched, fileName(kind.path));
          break;
        case "run_command":
          once(ran, kind.command);
          break;
        case "write_file":
        case "edit_file":
        case "delete_file":
          once(changed, fileName(kind.path));
          break;
        case "move_file":
          once(changed, fileName(kind.to));
          break;
      }
    }
  }
  const lines: LocalRead[] = [];
  for (const [path, count] of listed)
    lines.push({
      kind: "folder",
      text: count
        ? m.work_local_listed({ name: fileName(path), count })
        : m.work_local_read({ name: fileName(path) }),
    });
  if (read.length)
    lines.push({
      kind: "file",
      text:
        read.length === 1
          ? m.work_local_read({ name: read[0]! })
          : m.work_local_read_files({ count: read.length }),
    });
  if (searched.length)
    lines.push({ kind: "search", text: m.work_local_searched({ name: searched[0]! }) });
  if (ran.length)
    lines.push({
      kind: "command",
      text:
        ran.length === 1
          ? m.work_local_ran({ command: ran[0]! })
          : m.work_local_ran_commands({ count: ran.length }),
    });
  if (changed.length)
    lines.push({
      kind: "change",
      text:
        changed.length === 1
          ? m.work_local_changed({ name: changed[0]! })
          : m.work_local_changed_files({ count: changed.length }),
    });
  return lines.slice(0, 3);
}
