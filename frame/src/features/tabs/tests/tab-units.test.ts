import { describe, expect, it } from "vitest";
import type { TabView } from "$shared/ipc/bindings";
import { tabDisplayUnits } from "../lib/tab-units";

function tab(id: string): TabView {
  return {
    id,
    projection_revision: "00000000000000000000000000000001",
    title: `Tab ${id}`,
    url: `https://${id}.example/`,
    loading: false,
    can_go_back: false,
    can_go_forward: false,
    icon: null,
  };
}

function representedIds(
  units: ReturnType<typeof tabDisplayUnits>,
): Array<{ kind: "tab" | "split"; ids: string[] }> {
  return units.map((unit) => ({
    kind: unit.kind,
    ids: unit.kind === "tab" ? [unit.tab.id] : unit.tabs.map((candidate) => candidate.id),
  }));
}

describe("sidebar tab display units", () => {
  it("groups members once in native pane order at their earliest list position", () => {
    const tabs = [tab("a"), tab("b"), tab("c"), tab("d")];

    const units = tabDisplayUnits(tabs, { members: ["d", "b"] });

    expect(representedIds(units)).toEqual([
      { kind: "tab", ids: ["a"] },
      { kind: "split", ids: ["d", "b"] },
      { kind: "tab", ids: ["c"] },
    ]);
    expect(units[1]?.kind === "split" && units[1].tabs).toEqual([tabs[3], tabs[1]]);
  });

  it("renders independent rows when there is no retained group", () => {
    const tabs = [tab("a"), tab("b")];

    expect(representedIds(tabDisplayUnits(tabs, null))).toEqual([
      { kind: "tab", ids: ["a"] },
      { kind: "tab", ids: ["b"] },
    ]);
  });

  it.each([{ members: ["a"] }, { members: ["a", "a"] }, { members: ["a", "missing"] }])(
    "fails malformed membership closed to independent rows: $members",
    (group) => {
      const tabs = [tab("a"), tab("b")];

      expect(representedIds(tabDisplayUnits(tabs, group))).toEqual([
        { kind: "tab", ids: ["a"] },
        { kind: "tab", ids: ["b"] },
      ]);
    },
  );
});
