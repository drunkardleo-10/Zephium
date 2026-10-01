import type { WorkExecutionFact } from "$shared/ipc/bindings";
import { displayHost } from "$shared/ui/data/Artifact/artifact";
import { fileName } from "./work-files";
import * as m from "$shared/i18n/messages";

export type TimelineGlyph = "said" | "search" | "page" | "file" | "command" | "ask" | "done";
/** One step of a run as a person reads it: no ids, no payloads. */
export type TimelineRow = {
  key: string;
  glyph: TimelineGlyph;
  what: string;
  /** The host, file, query or command it acted on. */
  where: string;
  elapsed?: string;
  /** Why a step did not succeed, in Rust's words when it said any. */
  failure?: string;
  live: boolean;
};
/** Local, so the request lift does not pull the canvas model into its chunk. */
function clipText(value: string, max: number): string {
  if (value.length <= max) return value;
  const cut = value.slice(0, max);
  const last = cut.charCodeAt(max - 1);
  return last >= 0xd800 && last <= 0xdbff ? cut.slice(0, -1) : cut;
}
const WHERE_TEXT = 160;
const NOTE_TEXT = 280;

function elapsedLabel(millis: number): string {
  if (millis < 1000) return m.work_timeline_ms({ ms: Math.max(1, Math.round(millis)) });
  if (millis < 60_000) return m.work_timeline_seconds({ seconds: (millis / 1000).toFixed(1) });
  return m.work_timeline_minutes({ minutes: Math.round(millis / 60_000) });
}

type Step = NonNullable<WorkExecutionFact["steps"]>[number];
function describe(step: Step): Pick<TimelineRow, "glyph" | "what" | "where"> | null {
  const kind = step.kind;
  switch (kind.kind) {
    case "turn": {
      // A turn is a row only when the agent said something on it.
      const note = step.note?.trim();
      return note ? { glyph: "said", what: note, where: "" } : null;
    }
    case "search":
      return { glyph: "search", what: m.work_timeline_searched(), where: kind.query };
    case "read":
      return {
        glyph: "page",
        what: m.work_timeline_read(),
        where: displayHost(kind.url) || kind.url,
      };
    case "discover":
      return { glyph: "search", what: m.work_timeline_explored(), where: kind.query };
    case "publish":
      return { glyph: "done", what: m.work_timeline_placed(), where: "" };
    case "ask":
      return { glyph: "ask", what: m.work_timeline_asked(), where: kind.prompt };
    case "steer":
      return { glyph: "said", what: m.work_timeline_you_said(), where: kind.text };
    case "list":
      return { glyph: "file", what: m.work_timeline_looked_in(), where: fileName(kind.path) };
    case "read_file":
      return { glyph: "file", what: m.work_timeline_read(), where: fileName(kind.path) };
    case "search_files":
      return { glyph: "file", what: m.work_timeline_searched(), where: kind.query };
    case "write_file":
      return { glyph: "file", what: m.work_timeline_wrote(), where: fileName(kind.path) };
    case "edit_file":
      return { glyph: "file", what: m.work_timeline_changed(), where: fileName(kind.path) };
    case "move_file":
      return { glyph: "file", what: m.work_timeline_moved(), where: fileName(kind.to) };
    case "delete_file":
      return { glyph: "file", what: m.work_timeline_deleted(), where: fileName(kind.path) };
    case "run_command":
      return { glyph: "command", what: m.work_timeline_ran(), where: kind.command };
    case "finish":
      return { glyph: "done", what: m.work_timeline_finished(), where: "" };
    default:
      return null;
  }
}

/** The run as a quiet timeline: one row per step that did something a person can name. */
export function runTimeline(execution: WorkExecutionFact): TimelineRow[] {
  return (execution.steps ?? []).flatMap((step) => {
    const row = describe(step);
    if (!row) return [];
    const failed = step.status !== "succeeded" && step.status !== "running";
    const note = step.note?.trim();
    const millis = step.measurements?.wall_millis;
    return [
      {
        key: step.id,
        ...row,
        what: clipText(row.what, NOTE_TEXT),
        where: clipText(row.where, WHERE_TEXT),
        ...(millis ? { elapsed: elapsedLabel(millis) } : {}),
        ...(failed
          ? {
              failure: clipText(
                row.glyph !== "said" && note ? note : m.work_timeline_failed(),
                NOTE_TEXT,
              ),
            }
          : {}),
        live: step.status === "running",
      },
    ];
  });
}
