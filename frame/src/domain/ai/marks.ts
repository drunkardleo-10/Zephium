import type { IconSvgElement } from "@hugeicons/svelte";
import type { WorkModelProvider } from "$shared/ipc/bindings";
import ChatGptIcon from "@hugeicons/core-free-icons/ChatGptIcon";
import ClaudeIcon from "@hugeicons/core-free-icons/ClaudeIcon";
import DeepseekIcon from "@hugeicons/core-free-icons/DeepseekIcon";
import GoogleGeminiIcon from "@hugeicons/core-free-icons/GoogleGeminiIcon";
import ServerStack01Icon from "@hugeicons/core-free-icons/ServerStack01Icon";

/** OpenRouter has no Hugeicons glyph: one route forking into two, drawn to its grid. */
const OpenRouterMark: IconSvgElement = [
  ["path", { d: "M3 12H8.5", strokeLinecap: "round", key: "0" }],
  [
    "path",
    {
      d: "M8.5 12C11 12 11.5 7 15 7H20",
      strokeLinecap: "round",
      strokeLinejoin: "round",
      key: "1",
    },
  ],
  [
    "path",
    {
      d: "M8.5 12C11 12 11.5 17 15 17H20",
      strokeLinecap: "round",
      strokeLinejoin: "round",
      key: "2",
    },
  ],
  [
    "path",
    { d: "M17.5 4.5L20 7L17.5 9.5", strokeLinecap: "round", strokeLinejoin: "round", key: "3" },
  ],
  [
    "path",
    { d: "M17.5 14.5L20 17L17.5 19.5", strokeLinecap: "round", strokeLinejoin: "round", key: "4" },
  ],
];

/** Zephium's own symbol: two opposed hooks around a diamond, as on the app icon. */
const ZephiumMark: IconSvgElement = [
  [
    "path",
    { d: "M20 6H9C6.8 6 5 7.8 5 10", strokeLinecap: "round", strokeLinejoin: "round", key: "0" },
  ],
  [
    "path",
    {
      d: "M4 18H15C17.2 18 19 16.2 19 14",
      strokeLinecap: "round",
      strokeLinejoin: "round",
      key: "1",
    },
  ],
  ["path", { d: "M12 9.5L14.5 12L12 14.5L9.5 12Z", strokeLinejoin: "round", key: "2" }],
];

const marks: Record<WorkModelProvider, IconSvgElement> = {
  anthropic: ClaudeIcon,
  open_ai: ChatGptIcon,
  google: GoogleGeminiIcon,
  deep_seek: DeepseekIcon,
  open_router: OpenRouterMark,
  compatible: ServerStack01Icon,
  cloud: ZephiumMark,
};

export function providerMark(provider: WorkModelProvider): IconSvgElement {
  return marks[provider];
}
