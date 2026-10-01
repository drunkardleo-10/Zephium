import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import { serviceKey, type ServiceKey } from "$domain/connections";
import { stepsOf } from "./computer";

type CallState = "done" | "failed" | "waiting" | "declined" | "running";

/** A display-only view of one call a connection made. */
type ConnectionCall = {
  key: string;
  /** "Read issue #123". */
  text: string;
  /** What it was about: the issue's title, a count. */
  detail: string | null;
  state: CallState;
  url: string | null;
  /** The server it went through, as the call named it: "ticktick". */
  server: string | null;
};

export type ConnectionView = {
  service: ServiceKey;
  calls: ConnectionCall[];
  working: boolean;
};

const DONE: Record<string, [string, string]> = {
  list: ["Listed", "Listing"],
  get: ["Read", "Reading"],
  read: ["Read", "Reading"],
  fetch: ["Fetched", "Fetching"],
  find: ["Found", "Finding"],
  search: ["Searched", "Searching"],
  query: ["Queried", "Querying"],
  filter: ["Filtered", "Filtering"],
  create: ["Created", "Creating"],
  add: ["Added", "Adding"],
  update: ["Updated", "Updating"],
  edit: ["Edited", "Editing"],
  delete: ["Deleted", "Deleting"],
  remove: ["Removed", "Removing"],
  move: ["Moved", "Moving"],
  complete: ["Completed", "Completing"],
  send: ["Sent", "Sending"],
  post: ["Posted", "Posting"],
  reply: ["Replied to", "Replying to"],
  comment: ["Commented on", "Commenting on"],
  check: ["Checked", "Checking"],
  sync: ["Synced", "Syncing"],
  run: ["Ran", "Running"],
};

/**
 * A tool's name in a person's words: `list_projects` is "Listed projects"
 * once done and "Listing projects" while it runs; an unknown verb keeps the
 * tool's own words.
 */
export function toolWords(tool: string, running: boolean): string {
  const words = tool
    .replace(/^[a-z0-9-]+__/iu, "")
    .replace(/([a-z])([A-Z])/gu, "$1 $2")
    .split(/[\s_.-]+/u)
    .filter(Boolean)
    .map((word) => word.toLowerCase());
  if (!words.length) return tool;
  const verb = DONE[words[0]!];
  const rest = words.slice(1).join(" ");
  if (verb) return rest ? `${verb[running ? 1 : 0]} ${rest}` : verb[running ? 1 : 0];
  const said = words.join(" ");
  return said.charAt(0).toUpperCase() + said.slice(1);
}

/** "Read issue #123 · Crash on start" as the row's words and what they concern. */
function split(note: string): [string, string | null] {
  const at = note.indexOf(" · ");
  return at < 0 ? [note, null] : [note.slice(0, at), note.slice(at + 3)];
}

/** A connection part's view of its steps; the service comes from the part itself. */
export function connectionView(
  projection: WorkRuntimeProjection | undefined,
  ids: readonly string[],
  part: { title: string; host?: string },
): ConnectionView {
  const found = stepsOf(projection, ids);
  let connection: string | undefined;
  const calls: ConnectionCall[] = [];
  let working = false;
  for (const { run, step } of found) {
    if (step.part)
      connection ??=
        run.parts?.find((fact) => fact.id === step.part)?.service?.connection ?? undefined;
    if (step.status === "running") working = true;
    const kind = step.kind;
    // A call is its own step with its row in the note; earlier runs said it on a read.
    if (kind.kind === "call" || (kind.kind === "read" && step.note)) {
      const running = step.status === "running";
      const [said, detail] = split(step.note ?? "");
      // A bare "Used list_projects" says nothing a person reads: the tool's own name, in words.
      const text =
        kind.kind === "call" && (!said || /^Used \S+$/u.test(said))
          ? toolWords(kind.call.tool, running)
          : said;
      calls.push({
        key: step.id,
        text,
        detail,
        state: step.status === "succeeded" ? "done" : running ? "running" : "failed",
        url: kind.kind === "call" ? (kind.call.url ?? null) : kind.url,
        server: kind.kind === "call" ? kind.call.service : null,
      });
    } else if (kind.kind === "confirm") {
      const decision = kind.confirm.decision;
      // Once approved, the call it held shows as its own row.
      if (decision && decision !== "declined") continue;
      calls.push({
        key: step.id,
        text: kind.confirm.headline.replace(/\?$/u, ""),
        detail: null,
        state:
          decision === "declined" ? "declined" : step.status === "running" ? "waiting" : "failed",
        url: null,
        server: null,
      });
    }
  }
  return {
    service: serviceKey(connection, part.title, part.host),
    calls,
    working,
  };
}

/** Rows the view draws at full detail, so the canvas can size its slot. */
export function connectionRows(view: ConnectionView): number {
  return Math.max(1, Math.min(view.calls.length, 4) + (view.calls.length > 4 ? 1 : 0));
}
