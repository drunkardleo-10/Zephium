import type { WorkPartReasonV1 } from "$shared/ipc/bindings";
import type { PartNeed } from "../canvas-model";
import * as m from "$shared/i18n/messages";

/** What went wrong, said of the site it happened on: "Couldn’t read Kayak". */
function siteReason(name: string, reason: WorkPartReasonV1 | null | undefined): string {
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

/** What went wrong, said of the part, for the island that speaks for the whole run: "Flights couldn’t be read". */
function partReason(part: string, reason: WorkPartReasonV1 | null | undefined): string {
  switch (reason) {
    case "couldnt_read":
      return m.work_part_couldnt_read({ part });
    case "signed_out":
      return m.work_part_signed_out({ part });
    case "blocked_by_check":
      return m.work_part_blocked_by_check({ part });
    case "not_found":
      return m.work_part_not_found({ part });
    case "site_error":
      return m.work_part_site_error({ part });
    case "no_answer":
      return m.work_part_no_answer({ part });
    default:
      return m.work_part_unfinished({ part });
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
  const said = (site: string, reason: PartNeed["reason"]) =>
    where === "row" ? siteReason(site || part, reason) : partReason(part, reason);
  switch (need.kind) {
    case "sign_in":
      return {
        text:
          where === "row"
            ? m.work_need_sign_in({ site: need.target })
            : partReason(part, "signed_out"),
        action: m.work_ask_sign_in(),
      };
    case "allow_site":
      return { text: m.work_need_allow_site({ site: need.target }), action: m.work_ask_allow() };
    case "allow_folder":
      return { text: m.work_need_allow_folder({ name: need.target }), action: m.work_ask_allow() };
    case "use_connection":
      return {
        text: need.reason
          ? said(need.site ?? "", need.reason)
          : m.work_need_connection({ service: need.target }),
        action: m.work_need_use({ service: need.target }),
      };
    case "connect":
      return {
        text: m.work_need_connect({ service: need.target }),
        action: m.work_need_connect_action({ service: need.target }),
      };
    case "retry":
      return { text: said(need.target, need.reason), action: m.work_need_again() };
  }
}
