import { SvelteMap } from "svelte/reactivity";
import { tabs } from "$domain/tabs";
type Entry = { id: string; name: string; value: string };
type SessionDraft = {
  values: Record<string, string | boolean>;
  collections: Record<string, Entry[]>;
  photo: string | null;
};
const sessions = new SvelteMap<string, SessionDraft>();
function session() {
  const profile = tabs.profile()?.id ?? "initial";
  let draft = sessions.get(profile);
  if (!draft) {
    const created = $state<SessionDraft>({ values: {}, collections: {}, photo: null });
    draft = created;
    sessions.set(profile, draft);
  }
  return draft;
}
export const scope = () => tabs.profile()?.id ?? "initial";
export function get<T extends string | boolean>(
  key: string,
  initial: T,
): T extends boolean ? boolean : string {
  return (sessions.get(scope())?.values[key] ?? initial) as T extends boolean ? boolean : string;
}
export function set(key: string, value: string | boolean) {
  session().values[key] = value;
}
export function entries(key: string): Entry[] {
  return sessions.get(scope())?.collections[key] ?? [];
}
export function saveEntry(key: string, entry: Entry) {
  const items = entries(key);
  const index = items.findIndex((item) => item.id === entry.id);
  session().collections[key] =
    index < 0
      ? [...items, entry].slice(0, 100)
      : items.map((item) => (item.id === entry.id ? entry : item));
}
export function removeEntry(key: string, id: string) {
  session().collections[key] = entries(key).filter((item) => item.id !== id);
}
export const photo = () => sessions.get(scope())?.photo ?? null;
export function setPhoto(value: string | null) {
  session().photo = value;
}
export function reset() {
  sessions.delete(tabs.profile()?.id ?? "initial");
}

export function capture() {
  const source = sessions.get(scope());
  return {
    profile: scope(),
    draft: {
      values: { ...source?.values },
      collections: Object.fromEntries(
        Object.entries(source?.collections ?? {}).map(([key, items]) => [
          key,
          items.map((item) => ({ ...item })),
        ]),
      ),
      photo: source?.photo ?? null,
    },
  };
}
export function restore(snapshot: ReturnType<typeof capture>) {
  if (snapshot.profile !== scope()) return;
  const draft = $state<SessionDraft>(snapshot.draft);
  sessions.set(snapshot.profile, draft);
}
export function resetPrefixes(prefixes: string[]) {
  const draft = session();
  for (const key of Object.keys(draft.values))
    if (prefixes.some((prefix) => key.startsWith(prefix))) delete draft.values[key];
  for (const key of Object.keys(draft.collections))
    if (prefixes.some((prefix) => key.startsWith(prefix))) delete draft.collections[key];
}
