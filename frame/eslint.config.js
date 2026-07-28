import eslint from "@eslint/js";
import svelte from "eslint-plugin-svelte";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "src/shared/ipc/bindings.ts",
      "src/vite-env.d.ts",
      "svelte.config.js",
      "vite.config.ts",
    ],
  },
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  ...svelte.configs.recommended,
  ...svelte.configs.prettier,
  {
    files: ["**/*.ts", "**/*.svelte.ts"],
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        sourceType: "module",
      },
    },
  },
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.es2025,
      },
    },
    rules: {
      "no-console": ["error", { allow: ["error", "warn"] }],
      "@typescript-eslint/no-explicit-any": "off",
    },
  },
  {
    files: ["**/*.svelte"],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
      },
    },
  },
  {
    files: ["tests/**/*.ts"],
    languageOptions: {
      globals: globals.node,
    },
  },
);
