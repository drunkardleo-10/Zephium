import { describe, expect, it } from "vitest";
import {
  apiPermissionLabel,
  compatibilityLimitationLabel,
  hostPermissionLabel,
} from "../src/domain/extensions/permission-labels";

describe("extension permission copy", () => {
  it("keeps broad and local-file host access explicit", () => {
    expect(hostPermissionLabel("<all_urls>")).toBe("Read and change data on all websites");
    expect(hostPermissionLabel("file:///Users/example/*")).toBe(
      "Read and change data in matching local files",
    );
    expect(hostPermissionLabel("https://example.com/*")).toBe(
      "Read and change data on https://example.com/*",
    );
  });

  it("uses browser-owned copy instead of presenting unknown tokens as identity", () => {
    expect(apiPermissionLabel("clipboardRead")).toBe("Read copied content");
    expect(apiPermissionLabel("nativeMessaging")).toBe(
      "Use Zephium's restricted compatibility broker",
    );
    expect(apiPermissionLabel("sessions")).toBe("Restore the most recently closed tab");
    expect(apiPermissionLabel("futureCapability")).toBe(
      "Use the futureCapability browser capability",
    );
  });

  it("turns typed compatibility limits into specific browser-owned disclosures", () => {
    expect(compatibilityLimitationLabel({ type: "api_permission", name: "fontSettings" })).toBe(
      "Limited: Read and change browser font settings",
    );
    expect(compatibilityLimitationLabel({ type: "content_scripts" })).toBe(
      "Some page scripts have platform limitations",
    );
    expect(compatibilityLimitationLabel({ type: "native_messaging" })).toBe(
      "Arbitrary native app connections are unavailable",
    );
    expect(compatibilityLimitationLabel({ type: "declarative_net_request" })).toBe(
      "Some declarative network rules have platform limitations",
    );
  });
});
