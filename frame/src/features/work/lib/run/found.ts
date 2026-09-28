import type { Block, Board } from "../board/types";
import type { RunPart } from "./parts";
import { hostOf, siteKey } from "./site";

/** The sites a block's things come from, one per thing; empty when a thing names none. */
function thingSites(block: Block, board: Board): string[] | null {
  const entities =
    block.kind === "gallery" ? block.entities : block.kind === "entity" ? [block.entity] : null;
  if (!entities?.length) return null;
  const sites: string[] = [];
  for (const entity of entities) {
    const address =
      entity.homepage ??
      (entity.sources ?? []).map((key) => board.sources[key]?.url).find((url) => !!url);
    const host = address ? hostOf(address) : "";
    if (host) sites.push(siteKey(host));
  }
  // Most of the things must say where they're from for the set to be a part's.
  return sites.length * 2 >= entities.length ? sites : null;
}

/**
 * What each part found, for a run whose objects don't say: a set of things
 * that all come from the one site a part worked on sits at the end of that
 * part's row. Everything else is the result's.
 */
export function foundByPart(board: Board, parts: readonly RunPart[]): Map<string, string> {
  const byKey = new Map(
    parts.flatMap((part) =>
      part.helper === "browser" && part.pages.length ? [[part.key, part.id]] : [],
    ),
  );
  const found = new Map<string, string>();
  for (const block of board.blocks) {
    const sites = thingSites(block, board);
    if (!sites || new Set(sites).size !== 1) continue;
    const part = byKey.get(sites[0]!);
    if (part) found.set(block.id, part);
  }
  return found;
}
