import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

async function load(languages: string[]) {
  const root = { lang: "" };
  vi.stubGlobal("navigator", { languages, language: languages[0] });
  vi.stubGlobal("document", { documentElement: root });
  const locale = await import("../locale.svelte");
  const runtime = await import("$shared/i18n/runtime");
  return { locale, runtime, root };
}

describe("interface language", () => {
  it("follows the system to the first translated language, by region or base", async () => {
    const { locale, runtime, root } = await load(["xx-YY", "en-GB"]);
    locale.applyLanguage("system");
    expect(runtime.getLocale()).toBe("en");
    expect(root.lang).toBe("en");
  });

  it("falls back to the base language for anything untranslated", async () => {
    const { locale, runtime } = await load(["xx"]);
    locale.applyLanguage("system");
    expect(runtime.getLocale()).toBe(runtime.baseLocale);
    locale.applyLanguage("klingon");
    expect(runtime.getLocale()).toBe(runtime.baseLocale);
    locale.applyLanguage(null);
    expect(runtime.getLocale()).toBe(runtime.baseLocale);
  });

  it("offers every locale the catalog holds", async () => {
    const { locale, runtime } = await load(["en"]);
    expect(locale.languages()).toEqual(runtime.locales);
  });
});
