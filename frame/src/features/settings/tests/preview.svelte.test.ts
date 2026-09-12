import { beforeEach, describe, expect, it, vi } from "vitest";
const profile = vi.hoisted(() => ({ id: "first" }));
vi.mock("$domain/tabs/tabs.svelte", () => ({ profile: () => ({ id: profile.id }) }));
describe("session-only settings drafts", () => {
  beforeEach(() => {
    vi.resetModules();
    profile.id = "first";
  });
  it("keeps editing and cancellation separate from other profiles", async () => {
    const preview = await import("../lib/preview.svelte");
    preview.set("downloads.path", "Downloads");
    const checkpoint = preview.capture();
    preview.set("downloads.path", "Pictures");
    preview.restore(checkpoint);
    expect(preview.get("downloads.path", "")).toBe("Downloads");
    profile.id = "second";
    preview.set("downloads.path", "Documents");
    preview.restore(checkpoint);
    expect(preview.get("downloads.path", "")).toBe("Documents");
    profile.id = "first";
    expect(preview.get("downloads.path", "")).toBe("Downloads");
  });
  it("resets only the selected preview section", async () => {
    const preview = await import("../lib/preview.svelte");
    preview.set("ntp.clock", false);
    preview.set("languages.interface", "pl");
    preview.resetPrefixes(["ntp."]);
    expect(preview.get("ntp.clock", true)).toBe(true);
    expect(preview.get("languages.interface", "")).toBe("pl");
  });
});
