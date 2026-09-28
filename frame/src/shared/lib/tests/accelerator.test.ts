import { describe, expect, it } from "vitest";
import { acceleratorFrom, acceleratorKeys } from "../accelerator";

describe("accelerators", () => {
  it("translates every modifier and names keys by their label", () => {
    expect(acceleratorKeys("CmdOrCtrl+Shift+T", true)).toEqual(["⌘", "⇧", "T"]);
    expect(acceleratorKeys("Ctrl+Shift+Tab", true)).toEqual(["⌃", "⇧", "Tab"]);
    expect(acceleratorKeys("Ctrl+Alt+KeyK", true)).toEqual(["⌃", "⌥", "K"]);
    expect(acceleratorKeys("CmdOrCtrl+Shift+Space", false)).toEqual(["Ctrl", "Shift", "Space"]);
    expect(acceleratorKeys("", true)).toEqual([]);
  });

  it("spells a key press by physical key, and nothing for modifiers alone", () => {
    const press = (init: Partial<KeyboardEvent>) =>
      ({
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
        metaKey: false,
        ...init,
      }) as KeyboardEvent;
    expect(acceleratorFrom(press({ code: "Space", altKey: true, metaKey: true }), true)).toBe(
      "Alt+Cmd+Space",
    );
    expect(acceleratorFrom(press({ code: "KeyK", ctrlKey: true, shiftKey: true }), false)).toBe(
      "Ctrl+Shift+KeyK",
    );
    expect(acceleratorFrom(press({ code: "MetaLeft", metaKey: true }), true)).toBeNull();
  });
});
