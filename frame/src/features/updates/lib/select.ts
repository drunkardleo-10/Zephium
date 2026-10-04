import type { RuntimeSecurityAdvisory, UpdateStatus } from "$shared/ipc/bindings";
import { compareVersions, systemUpdateTarget, type SystemUpdateTarget } from "$domain/updates";

/** The update itself, waiting on a relaunch; never dismissed. */
export type UpdatePill = { kind: "ready"; version: string } | { kind: "installing" };
/** A notice to read once. */
export type UpdateCard =
  { kind: "security"; target: SystemUpdateTarget } | { kind: "updated"; version: string };

export type NoticeFacts = {
  status: UpdateStatus;
  relaunching: boolean;
  /** The running version; null until native has said. */
  version: string | null;
  /** The stored acknowledgements; null until read, "" when never written. */
  seen: string | null;
  securityDismissed: string | null;
  advisories: readonly RuntimeSecurityAdvisory[];
};

export type Notices = { pill: UpdatePill | null; card: UpdateCard | null };

function pillFor(status: UpdateStatus, relaunching: boolean): UpdatePill | null {
  if (status.state === "installing") return { kind: "installing" };
  if (status.state !== "ready") return null;
  return relaunching ? { kind: "installing" } : { kind: "ready", version: status.version };
}

/** Dismissed at an older Zephium, or never: a system still behind is
 *  mentioned again once per Zephium version. */
function securityDue(dismissed: string, version: string) {
  if (dismissed === "") return true;
  const order = compareVersions(dismissed, version);
  return order === null ? dismissed !== version : order < 0;
}

export function selectNotices(facts: NoticeFacts): Notices {
  // A build that cannot update (development, unsupported) shows none of this.
  if (facts.status.state === "unavailable") return { pill: null, card: null };
  const pill = pillFor(facts.status, facts.relaunching);
  const { version, seen, securityDismissed } = facts;
  if (version === null) return { pill, card: null };
  const target = systemUpdateTarget(facts.advisories);
  if (target !== null && securityDismissed !== null && securityDue(securityDismissed, version))
    return { pill, card: { kind: "security", target } };
  if (seen !== null && seen !== "" && seen !== version)
    return { pill, card: { kind: "updated", version } };
  return { pill, card: null };
}
