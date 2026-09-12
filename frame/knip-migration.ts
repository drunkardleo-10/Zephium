import type { KnipConfig } from "knip";

// 2026-09-11, steps 3 and 6: audit exports as modules gain public APIs;
// remove unused Work kit and migrate retained primitives. No new files are exempt.
const config = {
  ignoreIssues: {
    "src/features/essentials/components/EssentialsEmpty.svelte": ["files"],
    "src/features/settings/lib/preview.svelte.ts": ["exports"],
    "src/session/sidebar-mode.svelte.ts": ["exports"],
    "src/domain/tabs/tabs.svelte.ts": ["exports"],
    "src/domain/blocker/blocker.svelte.ts": ["exports", "types"],
    "src/domain/extensions/extensions.svelte.ts": ["exports"],
    "src/session/tools.svelte.ts": ["exports"],
    "src/domain/appearance/theme.ts": ["types"],
    "src/features/tabs/lib/sidebar-model.ts": ["types"],
    "src/features/settings/lib/catalog.ts": ["types"],
  },
} satisfies KnipConfig;
export const migrationIssues = config.ignoreIssues;
