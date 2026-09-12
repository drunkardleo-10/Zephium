import { fileURLToPath } from "node:url";

/** Shared by production Vite and both Vitest projects. Keep tsconfig paths in sync. */
export const alias = Object.fromEntries(
  ["app", "features", "session", "domain", "shared", "styles"].map((layer) => [
    `$${layer}`,
    fileURLToPath(new URL(`./src/${layer}`, import.meta.url)),
  ]),
);
