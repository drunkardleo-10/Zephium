import type {
  TabView,
  WorkContextSelectionV1,
  WorkEnvironmentSnapshot,
  WorkRuntimeProjection,
  NoteSummary,
} from "$shared/ipc/bindings";

const MAX_CONTEXT_ITEMS = 16;

/**
 * The selection as displayed, with the identity token Rust will recompute:
 * resource revision, tab URL, Work revision, or artifact id. Items whose token
 * is not yet known locally are left out rather than sent with a guess.
 */
export function contextSelection(
  snapshot: WorkEnvironmentSnapshot | null,
  selected: readonly string[],
  tabs: readonly TabView[],
  known: {
    notes: readonly NoteSummary[];
    objectives: ReadonlyMap<string, WorkRuntimeProjection>;
    /** Media record revisions: an image or a document is context like a note. */
    media?: ReadonlyMap<string, string>;
  },
  /** Consent, for this request, to list the window's open tabs: titles and addresses only. */
  openTabs = false,
): WorkContextSelectionV1 | null {
  if (!snapshot || (!selected.length && !openTabs)) return null;
  const items: WorkContextSelectionV1["items"] = [];
  for (const id of selected) {
    if (items.length >= MAX_CONTEXT_ITEMS) break;
    const element = snapshot.elements.find((element) => element.id === id);
    if (!element) continue;
    const reference = element.reference;
    let revision: string | null = null;
    switch (reference.kind) {
      case "resource":
        revision =
          known.notes.find((note) => note.id === reference.resource)?.revision ??
          known.media?.get(reference.resource) ??
          null;
        break;
      case "browser": {
        const tab = tabs.find((tab) => tab.id === reference.tab);
        revision = tab ? (tab.url ?? "") : null;
        break;
      }
      case "link":
        revision = reference.url;
        break;
      case "objective":
        revision = known.objectives.get(reference.objective)?.work.revision ?? null;
        break;
      case "artifact":
      case "subject":
      case "finding":
      case "source":
        revision = reference.artifact;
        break;
    }
    if (revision !== null) items.push({ element: id, revision });
  }
  if (!items.length && !openTabs) return null;
  return { environment: snapshot.id, items, ...(openTabs ? { tabs: true } : {}) };
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} KB`;
}
