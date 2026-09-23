import { commands } from "$shared/ipc/bindings";
import type {
  ResourceCall_Deserialize as ResourceCall,
  ResourceResponse_Serialize as ResourceResponse,
} from "$shared/ipc/bindings";
import { observe } from "$shared/lib/observe";

const RESPONSE_DEADLINE_MS = 9000;

/** One profile-checked resource call.
 *
 *  An unobserved outcome is reported as `outcome_unknown` rather than as a
 *  failure: aborting the observation never cancels a write native may already
 *  have applied, and a caller that treats the two alike will duplicate it.
 */
export async function resourceCall(
  profile: string,
  call: ResourceCall,
  signal?: AbortSignal,
): Promise<ResourceResponse> {
  const result = await observe(
    Promise.resolve().then(() => commands.resourceCall(profile, call)),
    RESPONSE_DEADLINE_MS,
    signal,
  );
  if (result.state !== "received") return { kind: "error", error: "outcome_unknown" };
  const reply = result.value;
  if (reply.profile !== profile && reply.response.kind !== "error")
    return { kind: "error", error: "unavailable" };
  return reply.response;
}
