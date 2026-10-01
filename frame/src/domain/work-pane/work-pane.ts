import { commands, type WorkPaneRect, type WorkPaneTarget } from "$shared/ipc/bindings";
import { settle, type Settlement } from "$domain/operations";

export type { WorkPaneRect, WorkPaneTarget };

/** Shows the pane over `target`; the rect is the chrome's measured hole in window coordinates. */
export const show = (target: WorkPaneTarget, rect: WorkPaneRect): Promise<Settlement> =>
  settle(commands.workPaneShow(target, rect));

export const hide = (): Promise<Settlement> => settle(commands.workPaneHide());

/** Replaceable geometry fact; Rust ignores a stale generation. */
export const setRect = (rect: WorkPaneRect, generation: number) =>
  void commands.workPaneSetRect(rect, generation);
