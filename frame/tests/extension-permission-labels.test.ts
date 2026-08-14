import { describe, expect, it } from "vitest";
import {
  apiPermissionLabel,
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
    expect(apiPermissionLabel("futureCapability")).toBe(
      "Use the futureCapability browser capability",
    );
  });
});
