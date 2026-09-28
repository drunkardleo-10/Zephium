import type { ObjectView, PicksView, SheetColumn, SheetView } from "../../lib/board/types";
import { diagramWidth } from "../../lib/diagram";

/** The size an object asks of the centre: wide for what spreads, a reading measure for text. */
export function centreSize(object: ObjectView): { width: number; height: number } {
  switch (object.kind) {
    case "diagram":
      return {
        width: Math.max(1040, Math.min(1600, diagramWidth(object.diagram) + 96)),
        height: 860,
      };
    case "sheet":
    case "picks":
      return { width: 1280, height: 860 };
    case "plot":
    case "plan":
    case "diff":
    case "code":
    case "media":
    case "page":
      return { width: 1040, height: 820 };
    case "document":
    case "reply":
    case "list":
    case "draft":
    case "note":
    case "file":
      return { width: 760, height: 860 };
    case "folder":
      return { width: 640, height: 640 };
  }
}

/**
 * Picks laid side by side as a sheet: one row per pick, its price, rating and
 * facts as typed columns in the order they first appear.
 */
export function picksSheet(picks: PicksView): SheetView {
  const labels: { label: string; kind: SheetColumn["kind"] }[] = [];
  for (const item of picks.items)
    for (const fact of item.facts)
      if (!labels.some((entry) => entry.label === fact.label))
        labels.push({
          label: fact.label,
          kind: fact.kind === "rating" ? "rating" : fact.kind === "text" ? "text" : "yes_no",
        });
  const priced = picks.items.some((item) => item.price);
  const rated = picks.items.some((item) => item.rating);
  const currency = picks.items.find((item) => item.price?.currency)?.price?.currency;
  const amounts = picks.items.every((item) => !item.price || item.price.amount !== undefined);
  const columns: SheetColumn[] = [
    { label: "", kind: "entity" },
    ...(priced
      ? [
          amounts && currency
            ? ({ label: "Price", kind: "money", currency, best: "min" } as const)
            : ({ label: "Price", kind: "text" } as const),
        ]
      : []),
    ...(rated ? [{ label: "Rating", kind: "rating", best: "max" } as const] : []),
    ...labels.slice(0, 10 - 1 - (priced ? 1 : 0) - (rated ? 1 : 0)),
  ];
  return {
    kind: "sheet",
    id: `${picks.id}:compare`,
    columns,
    rows: picks.items.map((item) => ({
      cells: columns.map((column, index) => {
        if (index === 0) return item.name;
        if (column.label === "Price" && priced)
          return column.kind === "money"
            ? String(item.price?.amount ?? "")
            : (item.price?.display ?? "");
        if (column.label === "Rating" && rated)
          return item.rating ? `${item.rating.value}/${item.rating.max}` : "";
        const fact = item.facts.find((entry) => entry.label === column.label);
        if (!fact) return column.kind === "yes_no" ? "unknown" : "";
        return column.kind === "yes_no"
          ? fact.kind === "text"
            ? "unknown"
            : fact.kind
          : fact.value;
      }),
      entity: {
        ...(item.picture ? { picture: item.picture } : {}),
        ...(item.logo ? { logo: item.logo } : {}),
      },
    })),
  };
}
