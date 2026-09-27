import { describe, expect, it } from "vitest";
import {
  apiPermissionLabel,
  compatibilityLimitationLabel,
  hostPermissionLabel,
} from "../permission-labels";

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
      "Communicate with approved desktop apps or Zephium compatibility services",
    );
    expect(apiPermissionLabel("sessions")).toBe("View and restore recently closed tabs");
    expect(apiPermissionLabel("futureCapability")).toBe(
      "Use the futureCapability browser capability",
    );
  });

  it("turns typed compatibility limits into specific browser-owned disclosures", () => {
    expect(apiPermissionLabel("identity")).toBe(
      "Sign in to an external service through the extension",
    );
    expect(compatibilityLimitationLabel({ type: "api_permission", name: "identity" })).toBe(
      "Interactive extension sign-in and Chrome account tokens are unavailable",
    );
    expect(compatibilityLimitationLabel({ type: "api_permission", name: "fontSettings" })).toBe(
      "Limited: Read and change browser font settings",
    );
    expect(compatibilityLimitationLabel({ type: "content_scripts" })).toBe(
      "Some page scripts have platform limitations",
    );
    expect(compatibilityLimitationLabel({ type: "main_document_content_scripts_only" })).toBe(
      "Some extension page scripts run only in top-level pages; embedded frames are unsupported",
    );
    expect(compatibilityLimitationLabel({ type: "fragment_url_content_scripts_unavailable" })).toBe(
      "These page scripts are unavailable when a page URL contains a # fragment",
    );
    expect(compatibilityLimitationLabel({ type: "content_script_fonts_unavailable" })).toBe(
      "Fonts supplied by these page scripts are unavailable",
    );
    expect(compatibilityLimitationLabel({ type: "side_panel_unavailable" })).toBe(
      "The extension side panel is unavailable",
    );
    expect(compatibilityLimitationLabel({ type: "offscreen_local_storage_only" })).toBe(
      "Offscreen documents support local storage only; other offscreen tasks are unavailable",
    );
    expect(compatibilityLimitationLabel({ type: "sandboxed_pages_unavailable" })).toBe(
      "Sandboxed extension widgets are unavailable",
    );
    expect(compatibilityLimitationLabel({ type: "clipboard_read_unavailable" })).toBe(
      "Reading copied content is unavailable, including automatic clipboard clearing and SSH key import",
    );
    expect(compatibilityLimitationLabel({ type: "native_messaging" })).toBe(
      "Arbitrary native app connections are unavailable",
    );
    expect(compatibilityLimitationLabel({ type: "declarative_net_request" })).toBe(
      "Some declarative network rules have platform limitations",
    );
    expect(
      compatibilityLimitationLabel({ type: "optional_api_unavailable", name: "identity" }),
    ).toBe("Optional extension sign-in is unavailable");
    expect(
      compatibilityLimitationLabel({
        type: "optional_host_unavailable",
        pattern: "https://a.test/*",
      }),
    ).toBe("Optional site access unavailable: https://a.test/*");
  });
});
