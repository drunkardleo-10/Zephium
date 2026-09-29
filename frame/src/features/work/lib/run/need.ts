import type { WorkPartReasonV1 } from "$shared/ipc/bindings";
import type { PartNeed } from "../canvas-model";
import * as m from "$shared/i18n/messages";

/** What went wrong, said of the thing it happened to: "Kayak couldn’t be read". */
function reasonText(name: string, reason: WorkPartReasonV1 | null | undefined): string {
  switch (reason) {
    case "couldnt_read":
      return m.work_reason_couldnt_read({ name });
    case "signed_out":
      return m.work_reason_signed_out({ name });
    case "blocked_by_check":
      return m.work_reason_blocked_by_check({ name });
    case "not_found":
      return m.work_reason_not_found({ name });
    case "site_error":
      return m.work_reason_site_error({ name });
    case "no_answer":
      return m.work_reason_no_answer({ name });
    default:
      return m.work_reason_unfinished({ name });
  }
}

/**
 * A part's need in a sentence and the one action that meets it. On its own
 * row the sentence names the site; the island names the part, since it
 * speaks for the whole run.
 */
export function needWords(
  need: PartNeed,
  part: string,
  where: "row" | "island",
): { text: string; action: string } {
  const name = where === "row" ? need.target || part : part;
  switch (need.kind) {
    case "sign_in":
      return where === "row"
        ? { text: m.work_need_sign_in({ site: need.target }), action: m.work_ask_sign_in() }
        : { text: m.work_reason_signed_out({ name }), action: m.work_ask_sign_in() };
    case "allow_site":
      return { text: m.work_need_allow_site({ site: need.target }), action: m.work_ask_allow() };
    case "allow_folder":
      return { text: m.work_need_allow_folder({ name: need.target }), action: m.work_ask_allow() };
    case "use_connection":
      return {
        text: need.reason
          ? reasonText(where === "row" ? need.site || part : part, need.reason)
          : m.work_need_connection({ service: need.target }),
        action: m.work_need_use({ service: need.target }),
      };
    case "retry":
      return { text: reasonText(name, need.reason), action: m.work_need_again() };
  }
}
