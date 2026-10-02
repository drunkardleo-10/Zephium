import { describe, expect, it } from "vitest";
import type { KeymapEntry } from "$shared/ipc/bindings";
import { commandTitle, keymapSections } from "../lib/keymap-model";

const entry = (id: string, group: KeymapEntry["group"], customizable = true): KeymapEntry => ({
  id,
  title: `native ${id}`,
  group,
  accelerator: null,
  default_accelerator: null,
  customizable,
  customized: false,
});

describe("keymap model", () => {
  it("titles commands in the interface language, numbering tab selection", () => {
    expect(commandTitle(entry("tab.new", "file"))).toBe("New tab");
    expect(commandTitle(entry("tab.select.3", "keys"))).toBe("Select tab 3");
    expect(commandTitle(entry("future.command", "file"))).toBe("native future.command");
  });

  it("lists rebindable commands by section and drops empty sections", () => {
    const sections = keymapSections([
      entry("browser.settings", "app"),
      entry("tab.new", "file"),
      entry("tab.select.1", "keys"),
      entry("tab.next", "window"),
      entry("tool.downloads", "window"),
      entry("launcher.toggle", "global", false),
      entry("work.pane.close", "work", false),
    ]);
    expect(sections.map((section) => section.id)).toEqual(["browser", "tabs"]);
    expect(sections[0]!.entries.map((e) => e.id)).toEqual([
      "browser.settings",
      "tab.new",
      "tool.downloads",
    ]);
    expect(sections[1]!.entries.map((e) => e.id)).toEqual(["tab.select.1", "tab.next"]);
  });
});
