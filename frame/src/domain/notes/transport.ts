import { commands } from "$shared/ipc/bindings";
import type { NoteCall, NoteResponse } from "$shared/ipc/bindings";
import { observe } from "$shared/lib/observe";

const RESPONSE_DEADLINE_MS = 9000;

/** One profile-checked notes call.
 *
 *  An unobserved outcome is `outcome_unknown`, never a failure: the notes
 *  thread may already have written the file, and a caller that treats the two
 *  alike would write it twice.
 */
export async function noteCall(
  profile: string,
  call: NoteCall,
  signal?: AbortSignal,
): Promise<NoteResponse> {
  const result = await observe(
    Promise.resolve().then(() => commands.noteCall(profile, call)),
    RESPONSE_DEADLINE_MS,
    signal,
  );
  if (result.state !== "received") return { kind: "error", error: "outcome_unknown" };
  const reply = result.value;
  if (reply.profile !== profile && reply.response.kind !== "error")
    return { kind: "error", error: "unavailable" };
  return reply.response;
}
