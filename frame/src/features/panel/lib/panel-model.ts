import type { PanelState } from "$shared/ipc/bindings";
const revision = /^[0-9a-f]{16}$/;
export function acceptPanelState(current: PanelState | null, next: PanelState): boolean {
  return (
    revision.test(next.revision) &&
    revision.test(next.session_id) &&
    (!current || next.revision > current.revision)
  );
}
