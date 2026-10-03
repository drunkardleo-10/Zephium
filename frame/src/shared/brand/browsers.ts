import type { BrandArt } from "$shared/ui/BrandMark";
import arc from "./marks/arc.svg";
import brave from "./marks/brave.svg";
import chrome from "./marks/chrome.svg";
import edge from "./marks/edge.svg";
import firefox from "./marks/firefox.svg";
import opera from "./marks/opera.svg";
import safari from "./marks/safari.svg";
import vivaldi from "./marks/vivaldi.svg";
import zen from "./marks/zen.svg";

const MARKS: Readonly<Record<string, BrandArt>> = {
  arc: { light: arc },
  brave: { light: brave },
  chrome: { light: chrome },
  edge: { light: edge },
  firefox: { light: firefox },
  opera: { light: opera },
  safari: { light: safari },
  vivaldi: { light: vivaldi },
  zen: { light: zen },
};

/** A browser's own mark, by the id an import source reports. A browser this
 *  build has no artwork for gets none, never another brand's. */
export function browserMark(id: string): BrandArt | null {
  return MARKS[id] ?? null;
}
