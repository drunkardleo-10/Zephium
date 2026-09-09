"use strict";

// Each case gets a fresh document/runtime; the candidate intentionally permits
// only one preparation opportunity per document. No native browser is involved.
const { spawnSync } = require("node:child_process");
const path = require("node:path");
const cases = ["normal", "cancel", "replace", "adopt", "retarget", "protected",
  "credential", "readonly", "rich", "focus-repurpose", "throw", "reentrant",
  "oversized", "input", "textarea", "nested-ancestors", "sibling-editability",
  "inherited-ancestors", "ancestor-change", "ancestor-focus-change", "ancestor-reparent", "ancestor-invalid",
  "sibling-tag-br", "sibling-tag-wbr", "sibling-tag-div", "sibling-tag-p", "sibling-tag-other",
  "div-rich", "div-nested", "div-identity", "div-editable", "div-structure", "div-sensitive",
  "post-root-identity", "post-root-editability", "post-root-writability", "post-target-structure",
  "command-false", "microtask-revert", "prepare-normal", "prepare-selection", "prepare-replace",
  "prepare-credential", "prepare-readonly", "prepare-spine", "prepare-occlusion", "prepare-denied"];
const root = path.resolve(__dirname, "../..");
for (const row of cases) {
  const result = spawnSync(process.execPath, [
    path.join(__dirname, "semantic-runtime-smoke-v1.js"),
    path.join(root, "crates/zephium-agentic/assets/semantic-runtime-v1.js"),
    `--isolated-command=${row}`
  ], { cwd: root, encoding: "utf8", timeout: 15000 });
  if (result.error || result.status !== 0) {
    process.stderr.write(result.stderr || String(result.error || "command smoke failed"));
    process.exit(1);
  }
  process.stdout.write(result.stdout);
}
for (const row of cases.filter(row => row.startsWith('prepare-') || ['input', 'textarea'].includes(row))) {
  const result = spawnSync(process.execPath, [
    path.join(__dirname, 'semantic-runtime-smoke-v1.js'),
    path.join(root, 'crates/zephium-agentic/assets/semantic-runtime-v1.js'),
    `--isolated-command=${row}`
  ], { cwd: root, encoding: 'utf8', timeout: 15000,
    env: { ...process.env, ZEPHIUM_LOCAL_ISOLATED_FILL_PREPARATION_ONLY_PROBE: '1' } });
  if (result.error || result.status !== 0) {
    process.stderr.write(result.stderr || String(result.error || 'preparation-only smoke failed'));
    process.exit(1);
  }
  process.stdout.write('preparation-only ' + result.stdout);
}
