import type { IconSvgElement } from "@hugeicons/svelte";
import {
  News01Icon,
  RefreshIcon,
  Shield01Icon,
  SparklesIcon,
  SystemUpdate01Icon,
} from "@hugeicons/core-free-icons";
import * as m from "$shared/i18n/messages";
import { commands } from "$shared/ipc/bindings";
import { IS_MAC } from "$shared/platform";
import type { SidebarCardAction } from "$shared/ui/SidebarCard";
import { releaseNotesUrl } from "$domain/updates";
import * as notices from "./notices.svelte";
import type { UpdateCard, UpdatePill } from "./select";

export type CardView = {
  key: string;
  title: string;
  detail?: string;
  icon: IconSvgElement;
  actions: SidebarCardAction[];
  dismiss: () => void;
};

export function cardView(card: UpdateCard): CardView {
  if (card.kind === "updated") {
    return {
      key: `updated:${card.version}`,
      title: m.update_done_title({ version: card.version }),
      icon: SparklesIcon,
      actions: [
        {
          label: m.update_whats_new(),
          icon: News01Icon,
          dismisses: true,
          onclick: () =>
            void commands.browserOpenUrl(releaseNotesUrl(card.version), true).catch(() => {}),
        },
      ],
      dismiss: notices.acknowledgeUpdate,
    };
  }
  if (card.target === "browser_runtime") {
    return {
      key: "security:browser_runtime",
      title: m.update_webview_title(),
      detail: m.update_webview_detail(),
      icon: Shield01Icon,
      actions: [],
      dismiss: notices.dismissSecurity,
    };
  }
  return {
    key: "security:operating_system",
    title: IS_MAC ? m.update_security_mac_title() : m.update_security_system_title(),
    detail: m.update_security_detail(),
    icon: Shield01Icon,
    actions: IS_MAC
      ? [
          {
            label: m.update_security_open(),
            icon: SystemUpdate01Icon,
            onclick: () => void commands.openSoftwareUpdate().catch(() => {}),
          },
        ]
      : [],
    dismiss: notices.dismissSecurity,
  };
}

export const PILL_ICON = RefreshIcon;

export const pillLabel = (pill: UpdatePill) =>
  pill.kind === "ready" ? m.update_relaunch() : m.update_installing();
