import type { IconSvgElement } from "@hugeicons/svelte";
import {
  CircleIcon,
  CubeIcon,
  Database01Icon,
  FlashIcon,
  Folder01Icon,
  GlobalIcon,
  LaptopIcon,
  LeftToRightListBulletIcon,
  Link04Icon,
  Settings02Icon,
  Shield01Icon,
  SparklesIcon,
} from "./icons";
/** A closed table: every part kind has its glyph, and an unknown one a dot. */
const GLYPHS: Record<string, IconSvgElement> = {
  client: LaptopIcon,
  edge: GlobalIcon,
  gateway: Shield01Icon,
  service: CubeIcon,
  worker: Settings02Icon,
  model: SparklesIcon,
  store: Database01Icon,
  queue: LeftToRightListBulletIcon,
  cache: FlashIcon,
  storage: Folder01Icon,
  external: Link04Icon,
  other: CircleIcon,
};
/** What a part is, drawn: its kind's glyph. */
export const diagramGlyph = (kind: string | undefined) => GLYPHS[kind ?? "other"] ?? CircleIcon;
