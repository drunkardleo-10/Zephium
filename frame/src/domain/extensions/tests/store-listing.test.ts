import { describe, expect, it } from "vitest";
import { isChromeStoreListing } from "../store-listing";

const id = "aeblfdkhhhdcdjpifhhbdiojplfjncoa";

describe("Chrome Web Store entry point", () => {
  it("recognizes current and legacy listing URLs without trusting lookalike origins", () => {
    expect(
      isChromeStoreListing(`https://chromewebstore.google.com/detail/1password/${id}?hl=en`),
    ).toBe(true);
    expect(isChromeStoreListing(`https://chrome.google.com/webstore/detail/${id}`)).toBe(true);
    for (const url of [
      `http://chromewebstore.google.com/detail/${id}`,
      `https://chromewebstore.google.com.evil.test/detail/${id}`,
      `https://user@chromewebstore.google.com/detail/${id}`,
      `https://chromewebstore.google.com:8443/detail/${id}`,
      `https://chromewebstore.google.com/detail/${id.slice(1)}`,
    ]) {
      expect(isChromeStoreListing(url)).toBe(false);
    }
  });
});
