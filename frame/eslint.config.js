import moduleRoles from "./module-roles.js";
import { architecture, primitiveArchitecture, ipcArchitecture } from "./architecture.config.js";
import eslint from "@eslint/js";
import svelte from "eslint-plugin-svelte";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "dist-work-preview/**",
      "project.inlang/cache/**",
      "src/shared/i18n/**",
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
  architecture,
  {
    files: ["src/features/**/*.{ts,svelte}"],
    plugins: { "zephium-modules": moduleRoles },
    rules: { "zephium-modules/structure": "error" },
  },
  primitiveArchitecture,
  ipcArchitecture,
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
      "@typescript-eslint/no-explicit-any": "error",
      // Svelte snippets may require an unused positional parameter before a used one.
      "@typescript-eslint/no-unused-vars": [
        "error",
        { varsIgnorePattern: "^_", argsIgnorePattern: "^_" },
      ],
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
    files: ["*.{js,ts}", "scripts/**/*.mjs", "src/**/*.test.ts"],
    languageOptions: {
      globals: globals.node,
    },
  },
);
