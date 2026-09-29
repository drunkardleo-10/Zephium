import type { WorkExecutionFact, WorkRuntimeProjection } from "$shared/ipc/bindings";
import { fileName } from "../work-files";

type Run = WorkExecutionFact;
type Step = NonNullable<Run["steps"]>[number];

/** How a changed file stands: proposed and waiting, written, or refused. */
type FileState = "waiting" | "writing" | "done" | "declined" | "failed";
type CommandState = "asking" | "running" | "passed" | "failed" | "stopped" | "declined";
export type Tests = { passed: number; failed: number };
export type Agent = "codex" | "claude";

/** A display-only view of one changed file. */
export type ComputerFile = {
  key: string;
  path: string;
  /** The folder it sits in, shortened, and its name. */
  folder: string;
  name: string;
  added: number;
  removed: number;
  state: FileState;
};

/** A display-only view of one command. */
export type ComputerCommand = {
  key: string;
  line: string;
  state: CommandState;
  ms: number | null;
  tests: Tests | null;
  /** The newest line of output while it runs. */
  live: string | null;
  /** A coding agent the command handed work to. */
  agent: Agent | null;
  /** The first test that failed, by its name. */
  failure: string | null;
  /** What a failed command said: its first line that reads as the error. */
  said: string | null;
  /** Looking around (`git status`, `ls`) rather than doing: its failure is only a fact. */
  inspection: boolean;
};

/** What a computer part did, from its steps: display only, never authority. */
export type ComputerView = {
  folder: string | null;
  files: ComputerFile[];
  commands: ComputerCommand[];
  reads: number;
  searches: number;
  /** The last test run's counts. */
  tests: Tests | null;
  working: boolean;
};

/** The runs of a projection that hold any of these steps. */
export function stepsOf(
  projection: WorkRuntimeProjection | undefined,
  ids: readonly string[],
): { run: Run; step: Step }[] {
  const wanted = new Set(ids);
  const found: { run: Run; step: Step }[] = [];
  for (const run of projection?.executions ?? [])
    for (const step of run.steps ?? []) if (wanted.has(step.id)) found.push({ run, step });
  return found;
}

/** Added and removed lines of a proposed change's diff. */
export function changeCounts(proposal: string | null | undefined): [number, number] {
  let added = 0;
  let removed = 0;
  for (const line of (proposal ?? "").split("\n")) {
    if (line.startsWith("+")) added++;
    else if (line.startsWith("-")) removed++;
  }
  return [added, removed];
}

/**
 * Test counts from the output people see: cargo, pytest, jest, vitest, go and
 * mocha summaries, as the helper's own summary reads them in Rust.
 */
export function testsIn(text: string): Tests | null {
  let passed = 0;
  let failed = 0;
  let seen = false;
  for (const match of text.matchAll(/test result: \w+\. (\d+) passed; (\d+) failed/gu)) {
    seen = true;
    passed += Number(match[1]);
    failed += Number(match[2]);
  }
  if (seen) return { passed, failed };
  const pytest = [...text.matchAll(/^=+ (.*\b(?:passed|failed)\b.*) in [\d.]+s/gmu)].at(-1);
  if (pytest) {
    const count = (word: string) =>
      Number(new RegExp(`(\\d+) ${word}`, "u").exec(pytest[1] ?? "")?.[1] ?? 0);
    return { passed: count("passed"), failed: count("failed") + count("errors?") };
  }
  const jest = /^Tests:\s+(?:(\d+) failed, )?(?:\d+ skipped, )?(?:(\d+) passed, )?\d+ total/mu.exec(
    text,
  );
  if (jest) return { passed: Number(jest[2] ?? 0), failed: Number(jest[1] ?? 0) };
  const vitest = /^\s*Tests\s+(?:(\d+) failed)?(?:\s*\|\s*)?(?:(\d+) passed)?/mu.exec(text);
  if (vitest && (vitest[1] || vitest[2]))
    return { passed: Number(vitest[2] ?? 0), failed: Number(vitest[1] ?? 0) };
  const go = [...text.matchAll(/^\s*--- (PASS|FAIL):/gmu)];
  if (go.length)
    return {
      passed: go.filter((m) => m[1] === "PASS").length,
      failed: go.filter((m) => m[1] === "FAIL").length,
    };
  const ran = [...text.matchAll(/^Ran (\d+) tests? in [\d.]+s/gmu)].at(-1);
  const end = [
    ...text.matchAll(
      /^(?:OK|FAILED)(?: \((?:failures=(\d+))?(?:, )?(?:errors=(\d+))?(?:, )?(?:skipped=(\d+))?[^)]*\))?\s*$/gmu,
    ),
  ].at(-1);
  if (ran && end) {
    const failed = Number(end[1] ?? 0) + Number(end[2] ?? 0);
    return { passed: Number(ran[1]) - failed - Number(end[3] ?? 0), failed };
  }
  const mocha = /^\s+(\d+) passing[^\n]*(?:\n\s+\d+ pending)?(?:\n\s+(\d+) failing)?/mu.exec(text);
  if (mocha) return { passed: Number(mocha[1]), failed: Number(mocha[2] ?? 0) };
  return null;
}

/**
 * The first failing test a runner names: cargo's `---- name stdout ----` or
 * `test name ... FAILED`, pytest's `FAILED path::name`, jest's and vitest's
 * `✕`/`×` lines, go's `--- FAIL: Name`, mocha's `1) name`.
 */
export function firstFailure(text: string): string | null {
  const patterns = [
    /^---- (\S+) stdout ----$/mu,
    /^test (\S+) \.\.\. FAILED$/mu,
    /^FAILED (\S+?)(?: - .*)?$/mu,
    /^\s*[✕×] (.+?)(?: \(\d+ ?m?s\))?$/mu,
    /^\s*--- FAIL: (\S+)/mu,
    /^\s+1\) (.+)$/mu,
  ];
  for (const pattern of patterns) {
    const match = pattern.exec(text);
    if (match?.[1]) return match[1].trim().slice(0, 120);
  }
  return null;
}

/** A failed command's own words: its first line that reads as an error, else its last line. */
export function errorLine(text: string): string | null {
  const lines = text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const error = lines.find((line) =>
    /\b(error|fatal|not found|no such|denied|cannot|can't|failed|not a)\b/iu.test(line),
  );
  const line = error ?? lines.at(-1);
  return line ? line.replace(/^(error|fatal):\s*/iu, "").slice(0, 160) : null;
}

/** Which coding agent a command hands work to. */
export function agentOf(line: string): Agent | null {
  const [program, verb] = line.trim().split(/\s+/u);
  if (program === "codex" && verb === "exec") return "codex";
  if (program === "claude" && verb === "-p") return "claude";
  return null;
}

/** What a coding agent is doing, from the newest event in its stream. */
function agentActivity(agent: Agent, output: string): string | null {
  const lines = output.trim().split("\n").reverse();
  for (const line of lines) {
    let event: Record<string, unknown>;
    try {
      event = JSON.parse(line) as Record<string, unknown>;
    } catch {
      continue;
    }
    if (agent === "codex" && event.type === "item.started") {
      const item = event.item as { type?: string; command?: string; changes?: { path?: string }[] };
      if (item.type === "command_execution" && item.command) return `$ ${item.command}`;
      if (item.type === "file_change" && item.changes?.[0]?.path)
        return `✎ ${fileName(item.changes[0].path)}`;
    }
    if (agent === "claude" && event.type === "assistant") {
      const content = (
        event.message as { content?: { type?: string; input?: { file_path?: string } }[] }
      )?.content;
      const tool = content?.findLast((block) => block.type === "tool_use");
      if (tool?.input?.file_path) return `✎ ${fileName(tool.input.file_path)}`;
    }
  }
  return null;
}

/** The newest non-empty line of a running command's output. */
function lastLine(text: string): string | null {
  const line = text
    .trimEnd()
    .split("\n")
    .findLast((l) => l.trim());
  return line ? line.trim().slice(0, 160) : null;
}

/** A path within the part's folder, as it reads best: `…/parser/lex.rs`. */
function shortFolder(path: string, root: string | null): string {
  const relative = root && path.startsWith(`${root}/`) ? path.slice(root.length + 1) : path;
  const parts = relative.split("/");
  parts.pop();
  if (!parts.length) return "";
  if (parts.length <= 2) return parts.join("/");
  return `…/${parts.slice(-2).join("/")}`;
}

function grantFolder(run: Run): string | null {
  for (const node of run.spec.nodes) {
    const capability = node.capability;
    if (capability.kind === "agent") return capability.grant.folders?.[0] ?? null;
  }
  return null;
}

function fileState(step: Step, decision: boolean | null | undefined): FileState {
  if (decision === false) return "declined";
  if (step.status === "succeeded") return "done";
  if (step.status === "failed" || step.status === "cancelled") return "failed";
  return decision === true ? "writing" : "waiting";
}

function commandState(step: Step, decision: boolean | null | undefined, tests: Tests | null) {
  const asks = step.local?.policy && step.local.policy.scope !== "none";
  if (decision === false) return "declined";
  if (step.status === "running") return asks && decision == null ? "asking" : "running";
  if (step.status === "succeeded") return tests?.failed ? "failed" : "passed";
  if (step.status === "cancelled" || step.note?.startsWith("Stopped")) return "stopped";
  return "failed";
}

/** The computer part's view of its steps. */
export function computerView(
  projection: WorkRuntimeProjection | undefined,
  ids: readonly string[],
): ComputerView {
  const found = stepsOf(projection, ids);
  const view: ComputerView = {
    folder: null,
    files: [],
    commands: [],
    reads: 0,
    searches: 0,
    tests: null,
    working: false,
  };
  const read = new Set<string>();
  const byPath = new Map<string, ComputerFile>();
  let root: string | null = null;
  for (const { run, step } of found) {
    root ??= step.local?.policy?.root ?? grantFolder(run);
    const kind = step.kind;
    if (step.status === "running") view.working = true;
    switch (kind.kind) {
      case "read_file":
      case "list":
        read.add(kind.path);
        break;
      case "search_files":
        view.searches++;
        break;
      case "write_file":
      case "edit_file":
      case "delete_file": {
        const [added, removed] = changeCounts(step.local?.proposal);
        const known = byPath.get(kind.path);
        const next: ComputerFile = {
          key: step.id,
          path: kind.path,
          folder: "",
          name: fileName(kind.path),
          added: (known?.state === "done" ? known.added : 0) + added,
          removed: (known?.state === "done" ? known.removed : 0) + removed,
          state: fileState(step, kind.decision),
        };
        // A declined or failed retry doesn't hide a change that landed.
        if (!known || known.state !== "done" || next.state === "done" || next.state === "waiting")
          byPath.set(
            kind.path,
            known?.state === "done" && next.state !== "done"
              ? { ...next, added: known.added + added, removed: known.removed + removed }
              : next,
          );
        break;
      }
      case "run_command": {
        const record = run.command_evidence?.find((r) => r.id === step.evidence)?.command;
        const output = record?.text ?? step.local?.output?.text ?? "";
        const tests = testsIn(output);
        const agent = agentOf(kind.command);
        const state = commandState(step, kind.decision, tests);
        const running = state === "running";
        const failed = state === "failed";
        view.commands.push({
          key: step.id,
          line: kind.command,
          state,
          ms: record?.elapsed_ms ?? step.local?.output?.elapsed_ms ?? null,
          tests,
          live: running ? (agent ? agentActivity(agent, output) : lastLine(output)) : null,
          agent,
          failure: tests?.failed ? firstFailure(output) : null,
          said: failed && !tests?.failed && !agent ? errorLine(output) : null,
          inspection: step.local?.policy?.reason === "inspection",
        });
        if (tests) view.tests = tests;
        break;
      }
      default:
        break;
    }
  }
  view.folder = root ? fileName(root) : null;
  view.files = [...byPath.values()].map((file) => ({
    ...file,
    folder: shortFolder(file.path, root),
  }));
  view.reads = read.size;
  return view;
}

/** Rows the view draws, so the canvas can size its slot before it measures itself. */
export function computerRows(view: ComputerView): number {
  const files = Math.min(view.files.length, 3);
  const commands = view.commands.reduce(
    (sum, command) =>
      sum + 1 + (command.tests || command.said || (command.live && !command.agent) ? 1 : 0),
    0,
  );
  const quiet = view.folder || view.reads || view.searches ? 1 : 0;
  return Math.max(1, files + commands + quiet);
}
