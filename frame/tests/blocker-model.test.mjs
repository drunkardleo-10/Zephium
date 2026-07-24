import assert from "node:assert/strict";
import test from "node:test";
import {
  canRefreshBlockerSources,
  diagnosticLabel,
  hasExactInstalledSourcePolicy,
  initialBlockerStatus,
  newestBlockerStatus,
  protectionLabel,
} from "../src/state/blocker-model.ts";

function revision(value) {
  return value.toString(16).padStart(32, "0");
}

test("revision reconciliation rejects stale and duplicate blocker status", () => {
  const current = { ...initialBlockerStatus(), projection_revision: revision(4) };
  const newer = {
    ...initialBlockerStatus(),
    projection_revision: revision(5),
    protection: "active",
  };
  const stale = {
    ...initialBlockerStatus(),
    projection_revision: revision(3),
    protection: "degraded",
  };

  assert.equal(newestBlockerStatus(current, newer), newer);
  assert.equal(newestBlockerStatus(current, current), current);
  assert.equal(
    newestBlockerStatus(current, { ...current, protection: "active" }),
    current,
  );
  assert.equal(newestBlockerStatus(current, stale), current);
  assert.equal(
    newestBlockerStatus(current, { ...newer, projection_revision: "5" }),
    current,
  );
  assert.equal(
    newestBlockerStatus(current, { ...newer, projection_revision: "g".repeat(32) }),
    current,
  );
});

test("initial status instances do not share mutable reconciliation state", () => {
  const first = initialBlockerStatus();
  const second = initialBlockerStatus();
  first.projection_revision = revision(9);
  assert.equal(second.projection_revision, revision(0));
  assert.equal(second.preference, "unavailable");
  assert.equal(second.source_phase, "not_configured");
  assert.equal(second.source_package_revision, null);
  assert.equal(second.source_identities, null);
  assert.equal(second.source_activation_pending, false);
});

test("source policy availability requires exact current and installed identities", () => {
  const manifest = "ab".repeat(32);
  const status = {
    ...initialBlockerStatus(),
    can_enable: true,
    source_phase: "fresh",
    source_package_revision: "0000000000000001",
    source_installed_revision: "0000000000000001",
    source_identities: {
      package_manifest_sha256: manifest,
      candidate_revision: null,
      candidate_manifest_sha256: null,
      installed_manifest_sha256: manifest,
    },
  };
  assert.equal(hasExactInstalledSourcePolicy(status), true);
  assert.equal(
    hasExactInstalledSourcePolicy({ ...status, source_installed_revision: "0000000000000002" }),
    false,
  );
  assert.equal(
    hasExactInstalledSourcePolicy({
      ...status,
      source_identities: { ...status.source_identities, installed_manifest_sha256: "cd".repeat(32) },
    }),
    false,
  );
  assert.equal(hasExactInstalledSourcePolicy({ ...status, source_activation_pending: true }), false);
  assert.equal(
    hasExactInstalledSourcePolicy({ ...status, source_material_repair_pending: true }),
    false,
  );
  assert.equal(
    hasExactInstalledSourcePolicy({ ...status, source_material_repair_retry_pending: true }),
    false,
  );
  assert.equal(hasExactInstalledSourcePolicy({ ...status, source_phase: "shutdown" }), false);
});

test("source refresh follows the actor-authoritative capability", () => {
  const unavailable = {
    ...initialBlockerStatus(),
    source_phase: "failed",
    source_activation_pending: true,
  };
  assert.equal(canRefreshBlockerSources(unavailable), false);
  assert.equal(
    canRefreshBlockerSources({
      ...unavailable,
      source_package_provenance: "tuf_repository",
      source_repair_retry_pending: true,
      can_refresh_sources: true,
    }),
    true,
  );
  assert.equal(
    canRefreshBlockerSources({
      ...unavailable,
      source_phase: "refreshing",
      source_repair_retry_pending: true,
      can_refresh_sources: false,
    }),
    false,
  );
  assert.equal(
    canRefreshBlockerSources({
      ...unavailable,
      source_phase: "failed",
      source_activation_pending: false,
      source_material_repair_retry_pending: true,
      can_refresh_sources: true,
    }),
    true,
  );
  assert.equal(
    canRefreshBlockerSources({
      ...unavailable,
      source_package_provenance: "release_bundle",
      can_refresh_sources: false,
    }),
    false,
  );
});

test("stable diagnostics enums receive bounded human labels", () => {
  assert.equal(protectionLabel("active"), "Protection active");
  assert.equal(protectionLabel("degraded"), "Protection degraded");
  assert.equal(diagnosticLabel("native_compilation"), "Native Compilation");
});
