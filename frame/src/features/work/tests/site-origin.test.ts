import { expect, test } from "vitest";
import { siteOrigin } from "../components/cards/HostGlyph.svelte";

test("a source's address or bare host becomes one HTTPS origin native may probe", () => {
  expect(siteOrigin("https://www.airbnb.co.uk/rooms/1?adults=1")).toBe("https://www.airbnb.co.uk");
  expect(siteOrigin("developers.openai.com")).toBe("https://developers.openai.com");
  expect(siteOrigin("http://example.org/page")).toBeNull();
  expect(siteOrigin("localhost")).toBeNull();
  expect(siteOrigin("  ")).toBeNull();
});
