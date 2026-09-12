import { styleMigrationOverrides } from "./stylelint-migration.js";
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
      rules: { "declaration-property-value-disallowed-list": null },
    },
  ],
  rules: {
    // Tailwind source() imports must use its supported string syntax.
    "import-notation": "string",
    "declaration-property-value-disallowed-list": {
      "/.*/": ["/#[0-9a-f]{3,8}\\b/i", "/\\b(?:rgb|rgba|hsl|hsla|oklch|oklab)\\(/i"],
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
