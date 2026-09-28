import { expect, test } from "vitest";
import { catalog, storeListing } from "../lib/catalog";

test("every catalog entry is a distinct Chrome Web Store extension", () => {
  const ids = catalog.map((entry) => entry.id);
  expect(new Set(ids).size).toBe(ids.length);
  for (const entry of catalog) {
    expect(entry.id).toMatch(/^[a-p]{32}$/);
    expect(entry.blurb().length).toBeGreaterThan(0);
    expect(storeListing(entry.id)).toBe(`https://chromewebstore.google.com/detail/${entry.id}`);
  }
});
