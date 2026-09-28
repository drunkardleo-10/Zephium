import { describe, expect, test } from "vitest";
import type { WorkModelEntry, WorkModelsV1 } from "$shared/ipc/bindings";
import { moreGroups, pickerGroups, shortName, usable } from "../catalog";
import { entry, models } from "$shared/testing/work-models";

describe("the picker's list", () => {
  test("groups recommended models of usable providers, Zephium first, and sets keyless apart", () => {
    const view = models({
      keys: { anthropic: "valid", open_ai: "set", google: "invalid" },
      cloud: true,
    });
    const { ready, needsKey } = pickerGroups(view, "lead");
    expect(ready.map((group) => group.provider)).toEqual(["cloud", "anthropic", "open_ai"]);
    expect(ready[1]!.entries.map((item) => item.id)).toEqual([
      "anthropic/claude-opus-5-5",
      "anthropic/claude-sonnet-5-5",
    ]);
    // A refused key reads as needing one, not as broken; OpenRouter and a
    // custom server have no recommended models to offer here.
    expect(needsKey).toEqual(["google", "deep_seek"]);
  });

  test("More models holds what the list leaves out, with provider listings, searchable", () => {
    const view = models({ keys: { open_router: "valid", anthropic: "set" } });
    const listed = { open_router: [entry("open_router", "moonshotai/kimi-k3", "Kimi K3", false)] };
    const groups = moreGroups(view, listed, "lead");
    expect(groups.map((group) => group.provider)).toEqual(["anthropic", "open_router"]);
    expect(groups[0]!.entries.map((item) => item.id)).toEqual(["anthropic/claude-fable-5-1"]);
    expect(moreGroups(view, listed, "lead", "kimi")[0]!.entries[0]!.display_name).toBe("Kimi K3");
    expect(moreGroups(view, listed, "lead", "nothing")).toEqual([]);
  });

  test("a custom server is usable once it has an address, keys only when set or valid", () => {
    const view: WorkModelsV1 = models({
      keys: { deep_seek: "invalid" },
      base: "http://localhost:11434/v1",
    });
    expect(usable(view, "compatible")).toBe(true);
    expect(usable(view, "deep_seek")).toBe(false);
    expect(usable(null, "anthropic")).toBe(false);
  });

  test("the trigger drops a family name the mark already says", () => {
    const opus: WorkModelEntry = entry("anthropic", "claude-opus-5-5", "Claude Opus 5.5", true);
    expect(shortName(opus)).toBe("Opus 5.5");
    expect(shortName(entry("open_ai", "gpt-6-sol", "GPT-6 Sol", true))).toBe("GPT-6 Sol");
  });
});
