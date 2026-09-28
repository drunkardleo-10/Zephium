import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
import { serviceKey, type ServiceKey } from "$domain/connections";
import { stepsOf } from "./computer";

type CallState = "done" | "failed" | "waiting" | "declined";

/** A display-only view of one call a connection made. */
type ConnectionCall = {
  key: string;
  /** "Read issue #123". */
  text: string;
  /** What it was about: the issue's title, a count. */
  detail: string | null;
  state: CallState;
  url: string | null;
};

export type ConnectionView = {
  service: ServiceKey;
  calls: ConnectionCall[];
  working: boolean;
};

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
    if ((kind.kind === "call" || kind.kind === "read") && step.note) {
      const [text, detail] = split(step.note);
      calls.push({
        key: step.id,
        text,
        detail,
        state:
          step.status === "succeeded" ? "done" : step.status === "running" ? "waiting" : "failed",
        url: kind.kind === "call" ? (kind.call.url ?? null) : kind.url,
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
