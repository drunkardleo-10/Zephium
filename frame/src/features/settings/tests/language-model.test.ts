import { describe, expect, it } from "vitest";
import { languages, normalizeLanguages, moveLanguage, readLanguages } from "../lib/language-model";
describe("language selection UI", () => {
  it("offers distinct language codes and common regional choices", () => {
    expect(languages.length).toBeGreaterThan(180);
    expect(new Set(languages.map((language) => language.code)).size).toBe(languages.length);
    for (const code of ["en-US", "en-GB", "pl", "ar", "ja", "zh-Hans", "uk", "cy"])
      expect(languages.some((language) => language.code === code)).toBe(true);
  });
  it("preserves preference order while rejecting duplicates and unknown codes", () => {
    expect(normalizeLanguages(["pl", "en-US", "pl", "invalid"])).toEqual(["pl", "en-US"]);
    expect(moveLanguage(["en-US", "pl", "de"], "de", -1)).toEqual(["en-US", "de", "pl"]);
    expect(moveLanguage(["en-US", "pl"], "en-US", -1)).toEqual(["en-US", "pl"]);
  });
  it("always keeps a valid language and bounds session drafts", () => {
    expect(readLanguages("not JSON")).toEqual(["en-US"]);
    expect(readLanguages('[null,"pl"]')).toEqual(["en-US"]);
    expect(normalizeLanguages([])).toEqual(["en-US"]);
    expect(normalizeLanguages(languages.map((language) => language.code))).toHaveLength(20);
  });
});
