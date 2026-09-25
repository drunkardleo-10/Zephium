import { expect, it } from "vitest";
import { checkBundleBudget } from "./bundle-budget";
it("checks CSS as well as JavaScript and rejects unbudgeted destinations", () => {
  const limits = { panel: { js: 100, css: 20 } };
  expect(checkBundleBudget("panel", { staticJsBytes: 100, staticCssBytes: 20 }, limits)).toBeNull();
  expect(checkBundleBudget("panel", { staticJsBytes: 100, staticCssBytes: 21 }, limits)).toContain(
    "CSS",
  );
  expect(checkBundleBudget("work", { staticJsBytes: 1, staticCssBytes: 0 }, limits)).toContain(
    "Unbudgeted",
  );
});
it("rejects invalid limits rather than disabling the ratchet", () => {
  expect(
    checkBundleBudget(
      "panel",
      { staticJsBytes: 1, staticCssBytes: 0 },
      { panel: { js: NaN, css: 0 } },
    ),
  ).not.toBeNull();
});
