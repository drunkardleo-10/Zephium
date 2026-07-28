import type { SplitGroupView, TabView } from "../../../shared/ipc/bindings";

export type TabDisplayUnit =
  | {
      key: string;
      kind: "tab";
      tab: TabView;
    }
  | {
      key: "split";
      kind: "split";
      tabs: TabView[];
    };

function independentTabs(tabs: readonly TabView[]): TabDisplayUnit[] {
  return tabs.map((tab) => ({ key: `tab:${tab.id}`, kind: "tab", tab }));
}

/**
 * Normalizes the Rust-owned split references into sidebar display units.
 *
 * A malformed group falls back to independent rows: partial grouping could
 * duplicate or remove the exact tab element inspected by the native
 * presentation barrier.
 */
export function tabDisplayUnits(
  tabs: readonly TabView[],
  splitGroup: SplitGroupView | null,
): TabDisplayUnit[] {
  if (splitGroup === null || splitGroup.members.length < 2) return independentTabs(tabs);

  const byId = new Map(tabs.map((tab, index) => [tab.id, { index, tab }] as const));
  const memberIds = new Set<string>();
  const members: TabView[] = [];
  let insertionIndex = tabs.length;

  for (const id of splitGroup.members) {
    const projected = byId.get(id);
    if (projected === undefined || memberIds.has(id)) return independentTabs(tabs);
    memberIds.add(id);
    members.push(projected.tab);
    insertionIndex = Math.min(insertionIndex, projected.index);
  }

  const units: TabDisplayUnit[] = [];
  tabs.forEach((tab, index) => {
    if (index === insertionIndex) units.push({ key: "split", kind: "split", tabs: members });
    if (!memberIds.has(tab.id)) units.push({ key: `tab:${tab.id}`, kind: "tab", tab });
  });
  return units;
}
