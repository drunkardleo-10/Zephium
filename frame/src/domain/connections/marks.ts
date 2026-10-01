import type { IconSvgElement } from "@hugeicons/svelte";
import ChatGptIcon from "@hugeicons/core-free-icons/ChatGptIcon";
import ClaudeIcon from "@hugeicons/core-free-icons/ClaudeIcon";
import Database01Icon from "@hugeicons/core-free-icons/Database01Icon";
import DiscordIcon from "@hugeicons/core-free-icons/DiscordIcon";
import DropboxIcon from "@hugeicons/core-free-icons/DropboxIcon";
import FigmaIcon from "@hugeicons/core-free-icons/FigmaIcon";
import Folder01Icon from "@hugeicons/core-free-icons/Folder01Icon";
import GithubIcon from "@hugeicons/core-free-icons/GithubIcon";
import GoogleDriveIcon from "@hugeicons/core-free-icons/GoogleDriveIcon";
import GoogleIcon from "@hugeicons/core-free-icons/GoogleIcon";
import Notion01Icon from "@hugeicons/core-free-icons/Notion01Icon";
import Plug01Icon from "@hugeicons/core-free-icons/Plug01Icon";
import SlackIcon from "@hugeicons/core-free-icons/SlackIcon";
import StripeIcon from "@hugeicons/core-free-icons/StripeIcon";
import TrelloIcon from "@hugeicons/core-free-icons/TrelloIcon";
import type { WorkServerV1 } from "$shared/ipc/bindings";

const round = { strokeLinecap: "round", strokeLinejoin: "round" } as const;

/** Git's mark: a square on its corner holding a branch. */
const GitMark: IconSvgElement = [
  [
    "path",
    {
      d: "M10.6 3.4L3.4 10.6C2.6 11.4 2.6 12.6 3.4 13.4L10.6 20.6C11.4 21.4 12.6 21.4 13.4 20.6L20.6 13.4C21.4 12.6 21.4 11.4 20.6 10.6L13.4 3.4C12.6 2.6 11.4 2.6 10.6 3.4Z",
      ...round,
      key: "0",
    },
  ],
  ["path", { d: "M9 7L15 13", ...round, key: "1" }],
  ["path", { d: "M12 10V16", ...round, key: "2" }],
];

/** Linear's mark: a disc whose lower corner is cut into three bands. */
const LinearMark: IconSvgElement = [
  ["path", { d: "M5.2 8.2A7.8 7.8 0 1 1 15.8 18.8L5.2 8.2Z", ...round, key: "0" }],
  ["path", { d: "M4.3 11.6L12.4 19.7", ...round, key: "1" }],
  ["path", { d: "M4.9 15.6L8.4 19.1", ...round, key: "2" }],
];

/** Gmail's mark: the envelope drawn as its M. */
const GmailMark: IconSvgElement = [
  [
    "path",
    {
      d: "M3 7.5C3 6.1 4.6 5.3 5.7 6.2L12 11L18.3 6.2C19.4 5.3 21 6.1 21 7.5V17C21 17.6 20.6 18 20 18H18V10L12 14.5L6 10V18H4C3.4 18 3 17.6 3 17V7.5Z",
      ...round,
      key: "0",
    },
  ],
];

/** Vercel's mark: the triangle. */
const VercelMark: IconSvgElement = [["path", { d: "M12 4L21 19.5H3L12 4Z", ...round, key: "0" }]];

/** A service's key, from a tool id, a site or a server's name, address or package. */
export type ServiceKey =
  | "github"
  | "git"
  | "codex"
  | "claude"
  | "slack"
  | "notion"
  | "linear"
  | "figma"
  | "google"
  | "gmail"
  | "drive"
  | "stripe"
  | "trello"
  | "discord"
  | "dropbox"
  | "vercel"
  | "database"
  | "files"
  | "other";

const MARKS: Record<ServiceKey, IconSvgElement> = {
  github: GithubIcon,
  git: GitMark,
  codex: ChatGptIcon,
  claude: ClaudeIcon,
  slack: SlackIcon,
  notion: Notion01Icon,
  linear: LinearMark,
  figma: FigmaIcon,
  google: GoogleIcon,
  gmail: GmailMark,
  drive: GoogleDriveIcon,
  stripe: StripeIcon,
  trello: TrelloIcon,
  discord: DiscordIcon,
  dropbox: DropboxIcon,
  vercel: VercelMark,
  database: Database01Icon,
  files: Folder01Icon,
  other: Plug01Icon,
};

const NAMES: Partial<Record<ServiceKey, string>> = {
  github: "GitHub",
  git: "Git",
  codex: "Codex",
  claude: "Claude Code",
  slack: "Slack",
  notion: "Notion",
  linear: "Linear",
  figma: "Figma",
  google: "Google",
  gmail: "Gmail",
  drive: "Google Drive",
  stripe: "Stripe",
  trello: "Trello",
  discord: "Discord",
  dropbox: "Dropbox",
  vercel: "Vercel",
};

/** Words that name a service, most specific first. */
const WORDS: [RegExp, ServiceKey][] = [
  [/\bgh\b|github/u, "github"],
  [/gmail/u, "gmail"],
  [/google[-_ ]?drive|\bgdrive\b/u, "drive"],
  [/google|gcal|calendar/u, "google"],
  [/\bgit\b/u, "git"],
  [/codex|openai/u, "codex"],
  [/claude|anthropic/u, "claude"],
  [/slack/u, "slack"],
  [/notion/u, "notion"],
  [/linear/u, "linear"],
  [/figma/u, "figma"],
  [/stripe/u, "stripe"],
  [/trello/u, "trello"],
  [/discord/u, "discord"],
  [/dropbox/u, "dropbox"],
  [/vercel/u, "vercel"],
  [/postgres|sqlite|mysql|supabase|database|\bsql\b/u, "database"],
  [/filesystem|\bfiles?\b/u, "files"],
];

/** The service a piece of text names, or `other`. */
export function serviceKey(...texts: (string | null | undefined)[]): ServiceKey {
  const text = texts.filter(Boolean).join(" ").toLowerCase();
  return WORDS.find(([pattern]) => pattern.test(text))?.[1] ?? "other";
}

/** The service a server is for: its id, name, address or package say it. */
export function serverKey(server: WorkServerV1): ServiceKey {
  const transport = server.transport;
  const where =
    transport.kind === "http"
      ? transport.url.replace(/^https?:\/\//u, "").split("/")[0]
      : [transport.command, ...transport.args].join(" ");
  return serviceKey(server.id, server.name, where);
}

export function serviceMark(key: ServiceKey): IconSvgElement {
  return MARKS[key];
}

/** A service's own name, when it has a well-known one. */
export function serviceName(key: ServiceKey): string | undefined {
  return NAMES[key];
}
