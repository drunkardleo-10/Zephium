import type { WorkExecutionFact } from "$shared/ipc/bindings";
import { agentDoing, endingNote } from "../agent-steps";
import { fileName } from "../work-files";
import * as m from "$shared/i18n/messages";

/** One closed fact of what a request's runs did, in the order a person would tell it. */
export type TrailIcon =
  | "search"
  | "page"
  | "knowledge"
  | "ask"
  | "steer"
  | "file"
  | "command"
  | "account"
  | "time"
  | "stopped"
  | "live";
export type TrailLine = {
  key: string;
  icon: TrailIcon;
  text: string;
  detail?: string;
  /** The step going on now: it is the one line that moves. */
  live?: boolean;
  /** The command record a line opens. */
  command?: string;
  /** Monospace: a command as it was typed. */
  code?: boolean;
};

const host = (url: string) => {
  try {
    return new URL(url).host.replace(/^www\./u, "");
  } catch {
    return "";
  }
};
const listed = (names: readonly string[], shown = 3) =>
  names.length <= shown
    ? names.join(", ")
    : `${names.slice(0, shown).join(", ")} ${m.work_trail_more({ count: names.length - shown })}`;

function elapsedText(millis: number): string {
  const seconds = Math.max(1, Math.round(millis / 1000));
  const minutes = Math.floor(seconds / 60);
  return minutes ? `${minutes}m ${String(seconds % 60).padStart(2, "0")}s` : `${seconds}s`;
}

/** What is going on now, in a word or a host. */
function now(execution: WorkExecutionFact): string {
  const doing = agentDoing(execution);
  const running = (execution.steps ?? []).filter((step) => step.status === "running");
  switch (doing) {
    case "searching":
      return m.work_line_searching();
    case "reading": {
      const read = running.find((step) => step.kind.kind === "read");
      const where = read?.kind.kind === "read" ? host(read.kind.url) : "";
      return where ? m.work_line_reading({ host: where }) : m.work_line_reading_web();
    }
    case "working": {
      for (const step of running) {
        const kind = step.kind;
        if (kind.kind === "read_file")
          return m.work_line_reading_file({ name: fileName(kind.path) });
        if (kind.kind === "write_file" || kind.kind === "edit_file")
          return m.work_line_writing_file({ name: fileName(kind.path) });
        if (kind.kind === "search_files") return m.work_line_searching_files();
      }
      return m.work_env_working();
    }
    case "writing":
      return m.work_line_writing();
    default:
      return m.work_line_thinking();
  }
}

/**
 * The request's trail: searches, pages read and not, what came from knowledge,
 * questions and answers, what the person said, files and commands, signed-in
 * sites, time on pages, and while a run goes on, what it is doing now.
 */
export function runTrail(executions: readonly WorkExecutionFact[], live: boolean): TrailLine[] {
  const lines: TrailLine[] = [];
  let searches = 0;
  let query = "";
  const read = new Map<string, boolean>();
  const unread = new Set<string>();
  const files = { read: new Set<string>(), changed: new Set<string>() };
  let wall = 0;
  let knowledge = false;
  const accounts = new Map<string, { used: number; pages: number }>();
  const asks: TrailLine[] = [];
  const commands: TrailLine[] = [];
  for (const execution of executions) {
    knowledge ||= execution.artifacts.some((artifact) => artifact.general_knowledge);
    for (const use of execution.accounts ?? [])
      accounts.set(use.host, { used: use.pages_used, pages: use.pages });
    for (const step of execution.steps ?? []) {
      wall += step.measurements?.wall_millis ?? 0;
      const kind = step.kind;
      switch (kind.kind) {
        case "search":
          if (step.status !== "cancelled") {
            searches += 1;
            query = kind.query;
          }
          break;
        case "read":
          if (step.status === "succeeded") read.set(kind.url, true);
          else if (step.status !== "running" && !read.has(kind.url)) unread.add(kind.url);
          break;
        case "read_file":
        case "search_files":
          if (step.status === "succeeded") files.read.add(kind.path);
          break;
        case "write_file":
        case "edit_file":
          if (step.status === "succeeded") files.changed.add(kind.path);
          break;
        case "ask":
          asks.push({
            key: `ask:${step.id}`,
            icon: "ask",
            text: kind.prompt,
            detail: kind.answer
              ? m.work_trail_you_answered({ answer: kind.answer })
              : step.status === "running"
                ? m.work_line_waiting_for_you()
                : m.work_trail_unanswered(),
          });
          break;
        case "steer":
          asks.push({
            key: `steer:${step.id}`,
            icon: "steer",
            text: m.work_trail_you_said({ text: kind.text }),
          });
          break;
        case "run_command": {
          const record = execution.command_evidence?.find((entry) => entry.id === step.evidence);
          // A command that never ran, declined or refused, is no part of what the run did.
          if (!record && step.status !== "running") break;
          const exit = record?.command.exit;
          const output = step.local?.output;
          // No output yet means no process: the command waits on the person.
          const waiting =
            step.status === "running" &&
            !output &&
            kind.decision == null &&
            step.local?.policy?.scope !== "none";
          const detail = record
            ? [
                typeof exit === "number" ? m.work_trail_exit({ code: exit }) : "",
                elapsedText(record.command.elapsed_ms),
              ]
                .filter(Boolean)
                .join(" · ")
            : waiting
              ? m.work_command_waiting()
              : (output?.text.trimEnd().split("\n").at(-1) ?? "");
          commands.push({
            key: `command:${step.id}`,
            icon: "command",
            text: kind.command,
            code: true,
            ...(step.status === "running" ? { live: true } : {}),
            ...(record ? { command: record.id } : {}),
            ...(detail ? { detail } : {}),
          });
          break;
        }
        default:
          break;
      }
    }
  }
  for (const url of read.keys()) unread.delete(url);
  if (searches)
    lines.push({
      key: "search",
      icon: "search",
      text:
        searches === 1 ? m.work_trail_searched_one() : m.work_trail_searched({ count: searches }),
      ...(query ? { detail: `“${query}”` } : {}),
    });
  if (read.size)
    lines.push({
      key: "pages",
      icon: "page",
      text: read.size === 1 ? m.work_trail_read_one() : m.work_trail_read({ count: read.size }),
      detail: listed([...new Set([...read.keys()].map(host).filter(Boolean))]),
    });
  if (unread.size)
    lines.push({
      key: "unread",
      icon: "page",
      text:
        unread.size === 1 ? m.work_trail_unread_one() : m.work_trail_unread({ count: unread.size }),
      detail: listed([...new Set([...unread].map(host).filter(Boolean))]),
    });
  if (knowledge)
    lines.push({
      key: "knowledge",
      icon: "knowledge",
      text: m.work_trail_knowledge(),
      ...(read.size ? {} : { detail: m.work_trail_knowledge_detail() }),
    });
  for (const [where, use] of accounts)
    lines.push({
      key: `account:${where}`,
      icon: "account",
      text: m.work_trail_as_you({ host: where }),
      detail: m.work_trail_pages_used({ used: use.used, pages: use.pages }),
    });
  if (files.read.size)
    lines.push({
      key: "files-read",
      icon: "file",
      text:
        files.read.size === 1
          ? m.work_trail_files_read_one()
          : m.work_trail_files_read({ count: files.read.size }),
      detail: listed([...files.read].map(fileName), 2),
    });
  if (files.changed.size)
    lines.push({
      key: "files-changed",
      icon: "file",
      text:
        files.changed.size === 1
          ? m.work_trail_files_changed_one()
          : m.work_trail_files_changed({ count: files.changed.size }),
      detail: listed([...files.changed].map(fileName), 2),
    });
  lines.push(...commands, ...asks);
  const last = executions.at(-1);
  if (wall && !live)
    lines.push({ key: "time", icon: "time", text: m.work_trail_time({ time: elapsedText(wall) }) });
  if (last && live) lines.push({ key: "now", icon: "live", text: now(last), live: true });
  else if (last && ["failed", "cancelled", "interrupted"].includes(last.status))
    lines.push({
      key: "stopped",
      icon: "stopped",
      text: last.status === "failed" ? m.work_trail_failed() : m.work_line_stopped(),
      ...(endingNote(last) ? { detail: endingNote(last)! } : {}),
    });
  return lines;
}
