import * as m from "$shared/i18n/messages";
import { IS_MAC, IS_WINDOWS } from "$shared/platform";

/** The file manager's own name for showing a file in its folder. */
export function revealLabel(): string {
  if (IS_MAC) return m.note_reveal();
  if (IS_WINDOWS) return m.note_reveal_explorer();
  return m.note_reveal_folder_generic();
}
