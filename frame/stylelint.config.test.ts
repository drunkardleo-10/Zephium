import { expect, it } from "vitest";
import stylelint from "stylelint";

it("requires semantic colors in new component styles", async () => {
  const result = await stylelint.lint({
    code: "<style>.probe { color: #fff; }</style>",
    codeFilename: `${import.meta.dirname}/src/shared/ui/Probe.svelte`,
    configFile: `${import.meta.dirname}/stylelint.config.js`,
  });
  expect(
    result.results.flatMap((result) => result.warnings.map((warning) => warning.rule)),
  ).toContain("declaration-property-value-disallowed-list");
});
