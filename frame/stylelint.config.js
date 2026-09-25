import { styleMigrationOverrides } from "./stylelint-migration.js";

const colorGuard = ["/#[0-9a-f]{3,8}\\b/i", "/\\b(?:rgb|rgba|hsl|hsla|oklch|oklab)\\(/i"];

/*
 * Shape is named by job, never by size: --radius-row, --radius-control,
 * --radius-card and so on. Before this rule existed 86% of border-radius call
 * sites named a size instead, which is how one product ended up with four
 * roundnesses for the same kind of object. Anything from 6px to 49px has to
 * come from a role token; below that is geometry rather than shape — a 2px
 * slider tip, a 3px text mark — and 0, 50% and inherit are structural.
 */
const radiusGuard = ["/\\b(?:[6-9]|[1-4][0-9])px\\b/", "/--radius-(?:xs|sm|md|lg|xl)\\b/"];
export default {
  extends: ["stylelint-config-standard"],
  ignoreFiles: ["src/shared/i18n/**"],
  overrides: [
    { files: ["**/*.svelte"], customSyntax: "postcss-html" },
    ...styleMigrationOverrides,
    {
      files: ["src/styles/tokens.css", "src/styles/tokens/**/*.css", "src/styles/axes/**/*.css"],
      rules: { "declaration-property-value-disallowed-list": null },
    },
    // Step 6: existing global color recipes migrate into token/axis ownership.
    {
      files: ["src/styles/browser.css", "src/styles/global.css"],
      rules: {
        "declaration-property-value-disallowed-list": { "border-radius": radiusGuard },
      },
    },
  ],
  rules: {
    // Tailwind source() imports must use its supported string syntax.
    "import-notation": "string",
    "declaration-property-value-disallowed-list": {
      "/.*/": colorGuard,
      "border-radius": radiusGuard,
    },
    "at-rule-no-unknown": [
      true,
      {
        ignoreAtRules: [
          "theme",
          "source",
          "utility",
          "variant",
          "custom-variant",
          "apply",
          "reference",
        ],
      },
    ],
    "selector-pseudo-class-no-unknown": [true, { ignorePseudoClasses: ["global"] }],
    "declaration-property-value-no-unknown": [true, { ignoreProperties: { "/^--/": ["/.*/"] } }],
  },
};
