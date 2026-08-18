import { describe, expect, it } from "vitest";
import {
  adjacentExtensionCenterSection,
  extensionProvenanceHost,
  extensionSourcePresentation,
  initialExtensionCenterSection,
  parseVerifiedCatalogDate,
} from "../src/domain/extensions/extension-presentation";

describe("extension source presentation", () => {
  it("preserves the authenticated lane independently from its timestamp", () => {
    expect(extensionSourcePresentation("zephium_verified", "1786924800")).toMatchObject({
      label: "Zephium Verified",
      tone: "verified",
    });
    expect(extensionSourcePresentation("external_compatibility", "1786924800")).toEqual({
      label: "External compatibility",
      verifiedAt: null,
      tone: "external",
    });
    expect(extensionSourcePresentation("developer_local", null)).toEqual({
      label: "Developer local",
      verifiedAt: null,
      tone: "developer",
    });
  });

  it("admits only canonical positive safe Unix seconds", () => {
    expect(parseVerifiedCatalogDate("1786924800")?.toISOString()).toBe("2026-08-17T00:00:00.000Z");
    for (const value of [null, "", "0", "01", "-1", "1.5", "not-a-time", "9007199254740992"]) {
      expect(parseVerifiedCatalogDate(value)).toBeNull();
    }
  });
});

describe("extension center section selection", () => {
  it("opens verified discovery for an empty profile and installed otherwise", () => {
    expect(initialExtensionCenterSection(0, 1)).toBe("verified");
    expect(initialExtensionCenterSection(0, 0)).toBe("installed");
    expect(initialExtensionCenterSection(1, 4)).toBe("installed");
  });

  it("moves between the two currently actionable sections", () => {
    expect(adjacentExtensionCenterSection("installed")).toBe("verified");
    expect(adjacentExtensionCenterSection("verified")).toBe("installed");
  });
});

describe("extension provenance presentation", () => {
  it("projects only an HTTPS hostname", () => {
    const provenance = {
      source_url: "https://github.com/philc/vimium/tree/revision",
      upstream_version: "2.4.2",
      license_expression: "MIT",
      attribution: "Vimium contributors",
    };
    expect(extensionProvenanceHost(provenance)).toBe("github.com");
    expect(
      extensionProvenanceHost({ ...provenance, source_url: "http://example.com/release" }),
    ).toBe(null);
    expect(extensionProvenanceHost({ ...provenance, source_url: "not a URL" })).toBeNull();
    expect(extensionProvenanceHost(null)).toBeNull();
  });
});
