import { describe, expect, test } from "vitest";
import type { WorkModelEntry, WorkModelsV1 } from "$shared/ipc/bindings";
import { pickerGroups, shortName, usable } from "../catalog";
import { entry, models } from "$shared/testing/work-models";

describe("the picker's list", () => {
  test("groups the curated models of usable providers, recommended first, Zephium first", () => {
    const view = models({
      keys: { anthropic: "valid", open_ai: "set", google: "invalid" },
      cloud: true,
    });
    const { ready, needsKey } = pickerGroups(view, "lead");
    expect(ready.map((group) => group.provider)).toEqual(["cloud", "anthropic", "open_ai"]);
    expect(ready[1]!.entries.map((item) => item.id)).toEqual([
      "anthropic/claude-opus-5-5",
      "anthropic/claude-sonnet-5-5",
      "anthropic/claude-fable-5-1",
    ]);
    expect(ready[2]!.entries.map((item) => item.id)).toEqual([
      "openai/gpt-6-luna",
      "openai/gpt-6-sol",
      "openai/gpt-6-astra",
    ]);
    // A refused key reads as needing one, not as broken; OpenRouter and a
    // custom server are optional and never ask.
    expect(needsKey).toEqual(["google", "deep_seek"]);
  });

  test("a custom server adds the models it serves", () => {
    const view = models({ base: "http://localhost:11434/v1" });
    const served = [entry("compatible", "qwen4:32b", "qwen4:32b", false)];
    const { ready } = pickerGroups(view, "lead", served);
    expect(ready.map((group) => group.provider)).toEqual(["compatible"]);
    expect(ready[0]!.entries[0]!.display_name).toBe("qwen4:32b");
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
