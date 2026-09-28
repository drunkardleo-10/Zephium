import type {
  WorkKeyStateV1,
  WorkModelEntry,
  WorkModelProvider,
  WorkModelRole,
  WorkModelsV1,
} from "$shared/ipc/bindings";

/** Providers that take the person's own key, in the order they are shown. */
export const keyedProviders: readonly WorkModelProvider[] = [
  "anthropic",
  "open_ai",
  "google",
  "deep_seek",
  "open_router",
  "compatible",
];

const names: Record<WorkModelProvider, string> = {
  anthropic: "Anthropic",
  open_ai: "OpenAI",
  google: "Google",
  deep_seek: "DeepSeek",
  open_router: "OpenRouter",
  compatible: "Custom",
  cloud: "Zephium",
};

export function providerName(provider: WorkModelProvider): string {
  return names[provider];
}

/** The model's name as the trigger shows it: the mark beside it says the family. */
export function shortName(entry: WorkModelEntry): string {
  const name = entry.display_name;
  for (const family of ["Claude ", "DeepSeek "]) {
    if (name.startsWith(family) && name.length > family.length) return name.slice(family.length);
  }
  return name;
}

export function keyState(models: WorkModelsV1 | null, provider: WorkModelProvider): WorkKeyStateV1 {
  return models?.providers.find((row) => row.provider === provider)?.key ?? "missing";
}

/** A provider can run calls now: its key is set and not refused. */
export function usable(models: WorkModelsV1 | null, provider: WorkModelProvider): boolean {
  if (!models) return false;
  if (provider === "cloud") return models.cloud.signed_in;
  if (provider === "compatible")
    return !!models.providers.find((row) => row.provider === "compatible")?.base;
  const key = keyState(models, provider);
  return key === "set" || key === "valid";
}

export type ModelGroup = { provider: WorkModelProvider; entries: WorkModelEntry[] };

/**
 * The picker's list for a role: recommended models of usable providers,
 * grouped by provider, Zephium first. Providers without a key are listed
 * apart, as needing one.
 */
export function pickerGroups(models: WorkModelsV1 | null, role: WorkModelRole) {
  const entries = models?.entries ?? [];
  const order: WorkModelProvider[] = ["cloud", ...keyedProviders];
  const ready: ModelGroup[] = [];
  const needsKey: WorkModelProvider[] = [];
  for (const provider of order) {
    const offered = entries.filter(
      (entry) =>
        entry.model.provider === provider && entry.recommended && entry.roles.includes(role),
    );
    if (usable(models, provider)) {
      if (offered.length) ready.push({ provider, entries: offered });
    } else if (provider !== "cloud" && provider !== "compatible" && provider !== "open_router") {
      needsKey.push(provider);
    }
  }
  return { ready, needsKey };
}

/** Everything else a usable provider offers for a role, grouped by provider. */
export function moreGroups(
  models: WorkModelsV1 | null,
  listed: Partial<Record<WorkModelProvider, WorkModelEntry[]>>,
  role: WorkModelRole,
  query = "",
): ModelGroup[] {
  const needle = query.trim().toLocaleLowerCase();
  const matches = (entry: WorkModelEntry) =>
    !needle ||
    entry.display_name.toLocaleLowerCase().includes(needle) ||
    entry.model.model.toLocaleLowerCase().includes(needle);
  const groups: ModelGroup[] = [];
  for (const provider of ["cloud", ...keyedProviders] as WorkModelProvider[]) {
    if (!usable(models, provider)) continue;
    const seen = new Set<string>();
    const rows: WorkModelEntry[] = [];
    for (const entry of [...(models?.entries ?? []), ...(listed[provider] ?? [])]) {
      if (entry.model.provider !== provider || seen.has(entry.id)) continue;
      seen.add(entry.id);
      if (entry.recommended || !entry.roles.includes(role) || !matches(entry)) continue;
      rows.push(entry);
    }
    if (rows.length) groups.push({ provider, entries: rows });
  }
  return groups;
}

export function entryOf(models: WorkModelsV1 | null, id: string | null | undefined) {
  if (!models || !id) return null;
  return models.entries.find((entry) => entry.id === id) ?? null;
}
