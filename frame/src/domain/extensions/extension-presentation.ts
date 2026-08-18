import type {
  ExtensionManagementProvenanceView,
  ExtensionManagementSourceView,
} from "../../shared/ipc/bindings";

export type ExtensionSourcePresentation = {
  label: string;
  verifiedAt: Date | null;
  tone: "verified" | "external" | "developer";
};

export type ExtensionCenterSection = "installed" | "verified";

export function initialExtensionCenterSection(
  installedCount: number,
  verifiedCount: number,
): ExtensionCenterSection {
  return installedCount === 0 && verifiedCount > 0 ? "verified" : "installed";
}

export function adjacentExtensionCenterSection(
  section: ExtensionCenterSection,
): ExtensionCenterSection {
  return section === "installed" ? "verified" : "installed";
}

/**
 * Converts browser-authenticated source data into inert presentation state.
 * Invalid timestamps never change the source label or mint a verification
 * claim; Rust owns the source/timestamp invariant and this boundary remains
 * defensive against malformed IPC during development.
 */
export function extensionSourcePresentation(
  source: ExtensionManagementSourceView,
  verifiedCatalogUnix: string | null,
): ExtensionSourcePresentation {
  switch (source) {
    case "zephium_verified":
      return {
        label: "Zephium Verified",
        verifiedAt: parseVerifiedCatalogDate(verifiedCatalogUnix),
        tone: "verified",
      };
    case "external_compatibility":
      return {
        label: "External compatibility",
        verifiedAt: null,
        tone: "external",
      };
    case "developer_local":
      return {
        label: "Developer local",
        verifiedAt: null,
        tone: "developer",
      };
  }
}

export function parseVerifiedCatalogDate(value: string | null): Date | null {
  if (value === null || !/^\d+$/.test(value)) return null;
  const seconds = Number(value);
  if (!Number.isSafeInteger(seconds) || seconds <= 0 || String(seconds) !== value) return null;
  const date = new Date(seconds * 1_000);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function extensionProvenanceHost(
  provenance: ExtensionManagementProvenanceView | null,
): string | null {
  if (provenance === null) return null;
  try {
    const source = new URL(provenance.source_url);
    return source.protocol === "https:" && source.hostname.length > 0 ? source.hostname : null;
  } catch {
    return null;
  }
}
