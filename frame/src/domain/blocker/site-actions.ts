import { commands } from "$shared/ipc/bindings";
import { boundedIpc, settleMutation } from "./blocker.svelte";

export const changeSite = (
  context: import("$shared/ipc/bindings").BlockerSiteContext,
  action: import("$shared/ipc/bindings").BlockerSiteAction,
) => settleMutation(() => commands.blockerSiteChange(context, action));

export async function picker(
  context: import("$shared/ipc/bindings").BlockerSiteContext,
  action: import("$shared/ipc/bindings").BlockerPickerAction,
): Promise<import("$shared/ipc/bindings").BlockerPickerView | null> {
  try {
    const result = await boundedIpc(commands.blockerPicker(context, action), 4_000);
    return result.status === "ok" ? result.data : null;
  } catch {
    return null;
  }
}
