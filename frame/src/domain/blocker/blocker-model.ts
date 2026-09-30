import type { BlockerProtection, BlockerStatusView } from "$shared/ipc/bindings";

export const BLOCKER_ZERO_REVISION = "00000000000000000000000000000000";
const PROJECTION_REVISION = /^[0-9a-f]{32}$/;
const SOURCE_REVISION = /^[0-9a-f]{16}$/;
const MANIFEST_SHA256 = /^[0-9a-f]{64}$/;
const SOURCE_UNAVAILABLE_PHASES = new Set<BlockerStatusView["source_phase"]>([
  "not_configured",
  "durable_activation_unsupported",
  "storage_unavailable",
  "clock_unsafe",
  "idle",
  "shutdown",
]);
export function initialBlockerStatus(): BlockerStatusView {
  return {
    site: null,
    projection_revision: BLOCKER_ZERO_REVISION,
    protection: "unavailable",
    phase: "unavailable",
    preference: "unavailable",
    config_revision: null,
    desired_enabled: null,
    applied_enabled: null,
    desired_generation: null,
    retained_generation: null,
    failure: null,
    retryable: false,
    retries_remaining: 0,
    applied_coverage: null,
    runtime_diagnostics: null,
    source_phase: "not_configured",
    source_failure: null,
    source_package_revision: null,
    source_installed_revision: null,
    source_package_provenance: null,
    source_installed_provenance: null,
    source_identities: null,
    source_package_created_unix: null,
    source_package_expires_unix: null,
    source_package_stale: null,
    source_refresh_due: false,
    source_count: null,
    source_bytes: null,
    source_activation_pending: false,
    source_material_repair_pending: false,
    source_material_repair_retry_pending: false,
    source_repair_retry_pending: false,
    source_last_refresh_attempt_unix: null,
    source_refresh_operation: null,
    can_enable: false,
    can_refresh_sources: false,
  };
}

export function newestBlockerStatus(
  current: BlockerStatusView,
  candidate: BlockerStatusView,
): BlockerStatusView {
  if (!PROJECTION_REVISION.test(candidate.projection_revision)) return current;
  if (!PROJECTION_REVISION.test(current.projection_revision)) return candidate;
  return candidate.projection_revision > current.projection_revision ? candidate : current;
}

export function hasExactInstalledSourcePolicy(status: BlockerStatusView): boolean {
  const identities = status.source_identities;
  const packageRevision = status.source_package_revision;
  const installedRevision = status.source_installed_revision;
  const packageManifest = identities?.package_manifest_sha256;
  const installedManifest = identities?.installed_manifest_sha256;
  return (
    status.can_enable &&
    !status.source_activation_pending &&
    !status.source_material_repair_pending &&
    !status.source_material_repair_retry_pending &&
    !SOURCE_UNAVAILABLE_PHASES.has(status.source_phase) &&
    packageRevision !== null &&
    installedRevision !== null &&
    SOURCE_REVISION.test(packageRevision) &&
    packageRevision === installedRevision &&
    packageManifest !== null &&
    packageManifest !== undefined &&
    installedManifest !== null &&
    installedManifest !== undefined &&
    MANIFEST_SHA256.test(packageManifest) &&
    packageManifest === installedManifest
  );
}

export function canRefreshBlockerSources(status: BlockerStatusView): boolean {
  return status.can_refresh_sources;
}

export function protectionLabel(protection: BlockerProtection): string {
  switch (protection) {
    case "disabled":
      return "Blocking off";
    case "pending":
      return "Preparing protection";
    case "active":
      return "Protection active";
    case "degraded":
      return "Protection degraded";
    case "unavailable":
      return "Protection unavailable";
  }
}

export type ShieldPresentation = {
  visible: boolean;
  tone: "quiet" | "warning";
  blocked: boolean;
  label: string;
};

/**
 * Chrome presentation of the focused profile's exact protection state. The
 * shield stays silent whenever protection is doing its job or the user turned
 * it off deliberately; only a degraded policy earns color.
 */
export function shieldPresentation(status: BlockerStatusView): ShieldPresentation {
  const label = protectionLabel(status.protection);
  switch (status.protection) {
    case "unavailable":
      return { visible: false, tone: "quiet", blocked: false, label };
    case "degraded":
      return { visible: true, tone: "warning", blocked: false, label };
    case "disabled":
      return { visible: true, tone: "quiet", blocked: true, label };
    case "pending":
    case "active":
      return { visible: true, tone: "quiet", blocked: false, label };
  }
}

export function diagnosticLabel(value: string): string {
  return value
    .split("_")
    .map((part) => `${part.slice(0, 1).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}

/** What the native "More" menu shows for the focused page, if anything. */
export function siteMenuState(status: BlockerStatusView): {
  siteProtected: boolean | null;
  canHide: boolean;
} {
  const site = status.site;
  if (!site || status.protection === "unavailable") return { siteProtected: null, canHide: false };
  const on = status.applied_enabled === true;
  return { siteProtected: on && !site.paused, canHide: on && site.ready && !site.busy };
}
