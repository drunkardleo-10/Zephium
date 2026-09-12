import { describe, expect, it } from "vitest";
import { ESLint } from "eslint";

const eslint = new ESLint({ cwd: import.meta.dirname });
async function violations(filePath: string, code: string) {
  const [result] = await eslint.lintText(code, { filePath });
  return result?.messages.map((message) => message.ruleId) ?? [];
}

describe("frontend architecture enforcement", () => {
  it("rejects upward imports and cross-feature dependencies through aliases", async () => {
    expect(
      await violations(
        "src/shared/ui/Probe.svelte",
        '<script>import Sidebar from "$features/sidebar/components/Sidebar.svelte";</script><Sidebar />',
      ),
    ).toContain("boundaries/dependencies");
    expect(
      await violations(
        "src/features/newtab/probe.ts",
        'export { default } from "$features/sidebar/components/Sidebar.svelte";',
      ),
    ).toContain("boundaries/dependencies");
  });
  it("rejects native bindings inside visual primitives", async () => {
    expect(
      await violations(
        "src/shared/ui/Probe.svelte",
        '<script>import { commands } from "$shared/ipc/bindings";</script>',
      ),
    ).toContain("no-restricted-imports");
  });
  it("rejects the caller-targeted event transport", async () => {
    expect(
      await violations(
        "src/domain/tabs/probe.ts",
        'export { listen } from "@tauri-apps/api/event";',
      ),
    ).toContain("no-restricted-imports");
  });
  it("rejects deep imports even from composition roots", async () => {
    expect(
      await violations(
        "src/app/Probe.svelte",
        '<script>import Sidebar from "$features/sidebar/components/Sidebar.svelte";</script>',
      ),
    ).toContain("boundaries/dependencies");
  });
  it("allows composition roots to assemble independent features", async () => {
    expect(
      await violations(
        "src/app/Probe.svelte",
        '<script>import { Sidebar } from "$features/sidebar";</script>',
      ),
    ).not.toContain("boundaries/dependencies");
  });
});

describe("feature module roles", () => {
  it("rejects behavior code importing a rendered component", async () => {
    expect(
      await violations(
        "src/features/launcher/lib/probe.ts",
        'export { default } from "../components/Launcher.svelte";',
      ),
    ).toContain("zephium-modules/structure");
  });
  it("rejects dynamic UI loading through a feature behavior module", async () => {
    expect(
      await violations(
        "src/features/launcher/lib/probe.ts",
        'export const load = () => import("../components/Launcher.svelte");',
      ),
    ).toContain("zephium-modules/structure");
  });
  it("keeps feature roots for the public API", async () => {
    expect(await violations("src/features/launcher/misc.ts", "export const value = 1;")).toContain(
      "zephium-modules/structure",
    );
  });
  it("allows tests to inspect their own behavior modules", async () => {
    expect(
      await violations(
        "src/features/launcher/tests/probe.test.ts",
        'export { sameSearch } from "../lib/search-model";',
      ),
    ).not.toContain("zephium-modules/structure");
  });
});
