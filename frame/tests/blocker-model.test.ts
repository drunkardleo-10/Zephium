import { describe, expect, it } from "vitest";
import {
  canRefreshBlockerSources,
  diagnosticLabel,
  hasExactInstalledSourcePolicy,
  initialBlockerStatus,
  newestBlockerStatus,
  protectionLabel,
} from "../src/domain/blocker/blocker-model";

function revision(value: number): string {
  return value.toString(16).padStart(32, "0");
}

describe("blocker projection admission", () => {
  it("rejects stale, duplicate, and malformed revisions", () => {
    const current = { ...initialBlockerStatus(), projection_revision: revision(4) };
    const newer = {
      ...initialBlockerStatus(),
      projection_revision: revision(5),
      protection: "active" as const,
    };
    const stale = {
      ...initialBlockerStatus(),
      projection_revision: revision(3),
      protection: "degraded" as const,
    };

    expect(newestBlockerStatus(current, newer)).toBe(newer);
    expect(newestBlockerStatus(current, current)).toBe(current);
    expect(newestBlockerStatus(current, { ...current, protection: "active" })).toBe(current);
    expect(newestBlockerStatus(current, stale)).toBe(current);
    expect(newestBlockerStatus(current, { ...newer, projection_revision: "5" })).toBe(current);
    expect(newestBlockerStatus(current, { ...newer, projection_revision: "g".repeat(32) })).toBe(
      current,
    );
  });

  it("creates independent initial status values", () => {
    const first = initialBlockerStatus();
    const second = initialBlockerStatus();
    first.projection_revision = revision(9);

    expect(second.projection_revision).toBe(revision(0));
    expect(second.preference).toBe("unavailable");
    expect(second.source_phase).toBe("not_configured");
    expect(second.source_package_revision).toBeNull();
    expect(second.source_identities).toBeNull();
    expect(second.source_activation_pending).toBe(false);
  });
});

describe("blocker source authority", () => {
  it("requires exact current and installed identities", () => {
    const manifest = "ab".repeat(32);
    const status = {
      ...initialBlockerStatus(),
      can_enable: true,
      source_phase: "fresh" as const,
      source_package_revision: "0000000000000001",
      source_installed_revision: "0000000000000001",
      source_identities: {
        package_manifest_sha256: manifest,
        candidate_revision: null,
        candidate_manifest_sha256: null,
        installed_manifest_sha256: manifest,
      },
    };

    expect(hasExactInstalledSourcePolicy(status)).toBe(true);
    expect(
      hasExactInstalledSourcePolicy({
        ...status,
        source_installed_revision: "0000000000000002",
      }),
    ).toBe(false);
    expect(
      hasExactInstalledSourcePolicy({
        ...status,
        source_identities: {
          ...status.source_identities,
          installed_manifest_sha256: "cd".repeat(32),
        },
      }),
    ).toBe(false);
    expect(hasExactInstalledSourcePolicy({ ...status, source_activation_pending: true })).toBe(
      false,
    );
    expect(hasExactInstalledSourcePolicy({ ...status, source_material_repair_pending: true })).toBe(
      false,
    );
    expect(
      hasExactInstalledSourcePolicy({
        ...status,
        source_material_repair_retry_pending: true,
      }),
    ).toBe(false);
    expect(hasExactInstalledSourcePolicy({ ...status, source_phase: "shutdown" })).toBe(false);
  });

  it("follows the actor-authoritative refresh capability", () => {
    const unavailable = {
      ...initialBlockerStatus(),
      source_phase: "failed" as const,
      source_activation_pending: true,
    };

    expect(canRefreshBlockerSources(unavailable)).toBe(false);
    expect(
      canRefreshBlockerSources({
        ...unavailable,
        source_package_provenance: "tuf_repository",
        source_repair_retry_pending: true,
        can_refresh_sources: true,
      }),
    ).toBe(true);
    expect(
      canRefreshBlockerSources({
        ...unavailable,
        source_phase: "refreshing",
        source_repair_retry_pending: true,
        can_refresh_sources: false,
      }),
    ).toBe(false);
    expect(
      canRefreshBlockerSources({
        ...unavailable,
        source_phase: "failed",
        source_activation_pending: false,
        source_material_repair_retry_pending: true,
        can_refresh_sources: true,
      }),
    ).toBe(true);
    expect(
      canRefreshBlockerSources({
        ...unavailable,
        source_package_provenance: "release_bundle",
        can_refresh_sources: false,
      }),
    ).toBe(false);
  });
});

it("maps stable diagnostic enums to bounded labels", () => {
  expect(protectionLabel("active")).toBe("Protection active");
  expect(protectionLabel("degraded")).toBe("Protection degraded");
  expect(diagnosticLabel("native_compilation")).toBe("Native Compilation");
});
