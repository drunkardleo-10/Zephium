// 2026-09-11, architecture step 6: remove each file override when its styles are scoped.
// These are the exact pre-migration violations, not a global relaxation.
export const styleMigrationOverrides = [
  {
    files: ["src/styles/browser.css"],
    rules: {
      "alpha-value-notation": null,
      "at-rule-empty-line-before": null,
      "comment-empty-line-before": null,
      "function-url-quotes": null,
      "media-feature-range-notation": null,
      "no-descending-specificity": null,
      "no-duplicate-selectors": null,
      "property-no-vendor-prefix": null,
      "rule-empty-line-before": null,
      "shorthand-property-no-redundant-values": null,
    },
  },
  {
    files: ["src/styles/controls.css"],
    rules: {
      "no-descending-specificity": null,
      "rule-empty-line-before": null,
    },
  },
  {
    files: ["src/styles/global.css"],
    rules: {
      "alpha-value-notation": null,
      "at-rule-empty-line-before": null,
      "import-notation": null,
      "rule-empty-line-before": null,
      "value-keyword-case": null,
    },
  },
  {
    files: ["src/styles/panel.css"],
    rules: {
      "at-rule-empty-line-before": null,
      "import-notation": null,
      "no-descending-specificity": null,
      "rule-empty-line-before": null,
    },
  },
  {
    files: ["src/styles/tokens.css"],
    rules: {
      "alpha-value-notation": null,
      "color-hex-length": null,
      "custom-property-empty-line-before": null,
      "rule-empty-line-before": null,
      "value-keyword-case": null,
    },
  },
  {
    files: ["src/features/tools/ToolSlot.svelte"],
    rules: {
      "rule-empty-line-before": null,
    },
  },
  {
    files: ["src/features/tools/tools.css"],
    rules: {
      "rule-empty-line-before": null,
    },
  },
];
