import * as m from "$shared/i18n/messages";
import { fields } from "./catalog";
import {
  Settings01Icon,
  PaintBoardIcon,
  Add01Icon,
  Layers01Icon,
  UserCircleIcon,
  Search01Icon,
  Shield01Icon,
  Key01Icon,
  Download01Icon,
  KeyboardIcon,
  Globe02Icon,
  SparklesIcon,
  AiBrowserIcon,
  Clock01Icon,
  DashboardSpeed01Icon,
  PuzzleIcon,
  InformationCircleIcon,
} from "@hugeicons/core-free-icons";
export const sections = [
  {
    id: "general",
    group: "browser",
    title: m.section_general,
    description: m.section_general_description,
    icon: Settings01Icon,
  },
  {
    id: "appearance",
    group: "personalize",
    title: m.section_appearance,
    description: m.section_appearance_description,
    icon: PaintBoardIcon,
  },
  {
    id: "newtab",
    group: "personalize",
    title: m.section_newtab,
    description: m.section_newtab_description,
    icon: Add01Icon,
  },
  {
    id: "tabs",
    group: "browser",
    title: m.section_tabs,
    description: m.section_tabs_description,
    icon: Layers01Icon,
  },
  {
    id: "profiles",
    group: "browser",
    title: m.section_profiles,
    description: m.section_profiles_description,
    icon: UserCircleIcon,
  },
  {
    id: "search",
    group: "browser",
    title: m.section_search,
    description: m.section_search_description,
    icon: Search01Icon,
  },
  {
    id: "privacy",
    group: "browser",
    title: m.section_privacy,
    description: m.section_privacy_description,
    icon: Shield01Icon,
  },
  {
    id: "passwords",
    group: "browser",
    title: m.section_passwords,
    description: m.section_passwords_description,
    icon: Key01Icon,
  },
  {
    id: "downloads",
    group: "browser",
    title: m.section_downloads,
    description: m.section_downloads_description,
    icon: Download01Icon,
  },
  {
    id: "shortcuts",
    group: "application",
    title: m.section_shortcuts,
    description: m.section_shortcuts_description,
    icon: KeyboardIcon,
  },
  {
    id: "languages",
    group: "application",
    title: m.section_languages,
    description: m.section_languages_description,
    icon: Globe02Icon,
  },
  {
    id: "work",
    group: "intelligence",
    title: m.section_work,
    description: m.section_work_description,
    icon: AiBrowserIcon,
  },
  {
    id: "ai",
    group: "intelligence",
    title: m.section_ai,
    description: m.section_ai_description,
    icon: SparklesIcon,
  },
  {
    id: "models",
    group: "intelligence",
    title: m.section_models,
    description: m.section_models_description,
    icon: Layers01Icon,
  },
  {
    id: "plugins",
    group: "intelligence",
    title: m.section_plugins,
    description: m.section_plugins_description,
    icon: PuzzleIcon,
  },
  {
    id: "mcp",
    group: "intelligence",
    title: m.section_mcp,
    description: m.section_mcp_description,
    icon: Globe02Icon,
  },
  {
    id: "skills",
    group: "intelligence",
    title: m.section_skills,
    description: m.section_skills_description,
    icon: SparklesIcon,
  },
  {
    id: "memory",
    group: "intelligence",
    title: m.section_memory,
    description: m.section_memory_description,
    icon: InformationCircleIcon,
  },
  {
    id: "focus",
    group: "intelligence",
    title: m.section_focus,
    description: m.section_focus_description,
    icon: Clock01Icon,
  },
  {
    id: "performance",
    group: "application",
    title: m.section_performance,
    description: m.section_performance_description,
    icon: DashboardSpeed01Icon,
  },
  {
    id: "account",
    group: "application",
    title: m.section_account,
    description: m.section_account_description,
    icon: UserCircleIcon,
  },
  {
    id: "developer",
    group: "application",
    title: m.section_developer,
    description: m.section_developer_description,
    icon: Key01Icon,
  },
  {
    id: "docs",
    group: "application",
    title: m.section_docs,
    description: m.section_docs_description,
    icon: InformationCircleIcon,
  },
  {
    id: "about",
    group: "application",
    title: m.section_about,
    description: m.section_about_description,
    icon: InformationCircleIcon,
  },
] as const;
export type SettingsSection = (typeof sections)[number]["id"];
export const groups = [
  { id: "browser", label: m.settings_browser },
  { id: "personalize", label: m.settings_personalize },
  { id: "intelligence", label: m.settings_intelligence },
  { id: "application", label: m.settings_application },
] as const;
export const emptySections = new Set<SettingsSection>([
  "ai",
  "models",
  "plugins",
  "mcp",
  "skills",
  "memory",
]);
export function searchSettings(query: string) {
  const words = query.trim().toLocaleLowerCase().split(/\s+/u);
  const matches = (text: string) => words.every((word) => text.toLocaleLowerCase().includes(word));
  const preferences = Object.entries(fields)
    .filter(
      ([, field]) =>
        !emptySections.has(field.section as SettingsSection) &&
        matches(field.label() + " " + field.description()),
    )
    .map(([id, field]) => ({
      id,
      section: field.section,
      label: field.label,
      description: field.description,
      target: id as string | null,
    }));
  const destinations = sections
    .filter(
      (section) =>
        matches(section.title()) && !preferences.some((field) => field.section === section.id),
    )
    .map((section) => ({
      id: `section:${section.id}`,
      section: section.id,
      label: section.title,
      description: section.description,
      target: null as string | null,
    }));
  return [...destinations, ...preferences];
}
export function searchSections(query: string) {
  const words = query.trim().toLocaleLowerCase().split(/\s+/u);
  return sections.filter((section) =>
    words.every((word) =>
      [
        section.title(),
        section.description(),
        ...Object.values(fields)
          .filter((f) => f.section === section.id)
          .flatMap((f) => [f.label(), f.description()]),
      ]
        .join(" ")
        .toLocaleLowerCase()
        .includes(word),
    ),
  );
}
