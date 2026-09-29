import type { ProjectEntry } from "../../lib/board/types";

/** An entry, or the count of entries a folder holds beyond those listed. */
export type TreeRow =
  | { name: string; folder: boolean; depth: number; holds?: number }
  | { more: number; depth: number };

/**
 * A project's structure as rows in reading order, folders before files. A
 * folder listed without its entries says how many it holds on its own row;
 * one listed with some is followed by them and a count of the rest.
 */
export function treeRows(tree: readonly ProjectEntry[], more?: number): TreeRow[] {
  const rows: TreeRow[] = [];
  const walk = (entries: readonly ProjectEntry[], depth: number, rest?: number) => {
    const sorted = [...entries].sort(
      (a, b) => Number(b.folder) - Number(a.folder) || a.name.localeCompare(b.name),
    );
    for (const entry of sorted) {
      const listed = !!entry.children?.length;
      rows.push({
        name: entry.name,
        folder: entry.folder,
        depth,
        ...(!listed && entry.more ? { holds: entry.more } : {}),
      });
      if (listed) walk(entry.children!, depth + 1, entry.more);
    }
    if (rest) rows.push({ more: rest, depth });
  };
  walk(tree, 0, more);
  return rows;
}
