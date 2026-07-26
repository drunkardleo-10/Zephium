import { describe, expect, it } from "vitest";
import { editingAddress, restingAddress } from "../src/features/sidebar/address-model";

describe("address presentation", () => {
  it("shows only the host while browser chrome is at rest", () => {
    expect(restingAddress("https://docs.example.com:8443/guide?q=1#top")).toBe(
      "docs.example.com:8443",
    );
  });

  it("preserves the complete authoritative URL only for explicit editing", () => {
    const url = "https://example.com/path?q=1";
    expect(editingAddress(url)).toBe(url);
  });

  it("fails closed for absent or invalid authoritative URLs", () => {
    expect(restingAddress(null)).toBe("");
    expect(restingAddress("not a url")).toBe("");
  });
});
