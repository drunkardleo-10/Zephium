import { SvelteMap } from "svelte/reactivity";

export const NOTE_HUES = ["lemon", "sky", "mint", "peach", "lilac", "rose"] as const;
export type NoteHue = (typeof NOTE_HUES)[number];

const KEY = "zephium.work.note-hues";
const chosen = new SvelteMap<string, NoteHue>();
try {
  const saved = JSON.parse(globalThis.localStorage?.getItem(KEY) ?? "{}") as Record<string, string>;
  for (const [id, hue] of Object.entries(saved))
    if ((NOTE_HUES as readonly string[]).includes(hue)) chosen.set(id, hue as NoteHue);
} catch {
  // A private window or cleared storage: every note keeps its cycled hue.
}

/** A note's soft hue: the one the person chose, else one of six by its id, the same every time. */
export function noteHue(id: string): NoteHue {
  const own = chosen.get(id);
  if (own) return own;
  let hash = 0;
  for (const char of id) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return NOTE_HUES[hash % NOTE_HUES.length]!;
}

export function chooseNoteHue(id: string, hue: NoteHue) {
  chosen.set(id, hue);
  try {
    globalThis.localStorage?.setItem(KEY, JSON.stringify(Object.fromEntries(chosen)));
  } catch {
    // Kept for this session only.
  }
}
