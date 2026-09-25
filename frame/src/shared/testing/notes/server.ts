import type {
  ChangedNote,
  NoteCall,
  NoteRecord,
  NoteReply,
  NoteResponse,
  NoteSummary,
} from "../../ipc/bindings";

/** Content revision: stable for equal bytes, like the native one. */
function revision(markdown: string): string {
  let hash = 0x811c9dc5;
  for (const character of markdown) {
    hash ^= character.codePointAt(0)!;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0").repeat(4);
}

/** Text after the title with Markdown syntax removed, roughly as the native
 *  index previews it. */
function preview(markdown: string): string {
  return markdown
    .split("\n")
    .slice(/^\s*#/u.test(markdown) ? 1 : 0)
    .map((line) =>
      line
        .replace(/^\s{0,3}(?:#{1,6}\s|[-+*]\s(?:\[[ xX]\]\s)?|\d+[.)]\s|>\s?)/u, "")
        .replace(
          /\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/gu,
          (_all, target: string, alias?: string) => alias ?? target,
        )
        .replace(/\[([^\]]*)\]\([^)]*\)/gu, "$1")
        .replace(/[*_~`]/gu, "")
        .trim(),
    )
    .filter(Boolean)
    .join(" ")
    .slice(0, 180);
}

function title(markdown: string, fallback: string): string {
  const heading = /^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$/mu.exec(
    markdown.trimStart().split("\n")[0] ?? "",
  );
  return heading?.[1] ?? fallback;
}

let ids = 0;
function id(): string {
  ids++;
  return `01J9ZQ3V6Q4M8Y2K7T5R${String(ids).padStart(6, "0")}`;
}

type Stored = { summary: NoteSummary; markdown: string };

/** Notes as the native service keeps them, in memory: revision-checked
 *  writes, idempotent creation, trash, and change events. */
export function notesTestServer(
  profile: string,
  emit?: (notes: ChangedNote[], reset: boolean) => void,
) {
  const notes = new Map<string, Stored>();
  const receipts = new Map<string, string>();
  let clock = 1_800_000_000_000;
  const calls: NoteCall[] = [];

  function store(
    existing: Stored | undefined,
    markdown: string,
    fields: Partial<NoteSummary> = {},
  ): Stored {
    const identity = existing?.summary.id ?? id();
    const fallback = existing ? existing.summary.path.replace(/\.md$/u, "") : "Untitled";
    const heading = title(markdown, fallback);
    const summary: NoteSummary = {
      id: identity,
      revision: revision(markdown),
      title: heading,
      preview: preview(markdown),
      pinned: existing?.summary.pinned ?? false,
      trashed: existing?.summary.trashed ?? false,
      editable: true,
      created_at: existing?.summary.created_at ?? String(clock),
      modified_at: String((clock += 1000)),
      path: existing?.summary.path ?? `${heading}.md`,
      ...fields,
    };
    const stored = { summary, markdown };
    notes.set(identity, stored);
    return stored;
  }

  function changed(id: string, reset = true) {
    emit?.([{ id, revision: notes.get(id)?.summary.revision ?? null }], reset);
  }

  function respond(call: NoteCall): NoteResponse {
    switch (call.kind) {
      case "list": {
        const search = call.query.search.toLowerCase();
        const items = [...notes.values()]
          .filter((note) => note.summary.trashed === call.query.trashed)
          .filter(
            (note) =>
              !search ||
              note.markdown.toLowerCase().includes(search) ||
              note.summary.title.toLowerCase().includes(search),
          )
          .map((note) => note.summary)
          .sort(
            (a, b) =>
              Number(b.pinned) - Number(a.pinned) || Number(b.modified_at) - Number(a.modified_at),
          );
        return { kind: "page", items: items.slice(0, call.query.limit), next: null };
      }
      case "get": {
        const note = notes.get(call.id);
        return note
          ? { kind: "record", record: structuredClone(note) }
          : { kind: "error", error: "not_found" };
      }
      case "create": {
        const known = receipts.get(call.request_id);
        if (known && notes.has(known))
          return {
            kind: "applied",
            request_id: call.request_id,
            summary: notes.get(known)!.summary,
          };
        const stored = store(undefined, call.markdown);
        receipts.set(call.request_id, stored.summary.id);
        changed(stored.summary.id);
        return { kind: "applied", request_id: call.request_id, summary: stored.summary };
      }
      case "write": {
        const note = notes.get(call.id);
        if (!note) return { kind: "error", error: "not_found" };
        if (note.summary.trashed || !note.summary.editable)
          return { kind: "error", error: "read_only" };
        if (note.summary.revision === revision(call.markdown))
          return { kind: "applied", request_id: call.request_id, summary: note.summary };
        if (note.summary.revision !== call.base_revision)
          return { kind: "conflict", current: structuredClone(note) as NoteRecord };
        const title = note.summary.title;
        const stored = store(note, call.markdown);
        changed(call.id, stored.summary.title !== title);
        return { kind: "applied", request_id: call.request_id, summary: stored.summary };
      }
      case "set_pinned": {
        const note = notes.get(call.id);
        if (!note) return { kind: "error", error: "not_found" };
        note.summary = { ...note.summary, pinned: call.pinned };
        changed(call.id);
        return { kind: "applied", request_id: "", summary: note.summary };
      }
      case "trash":
      case "restore": {
        const note = notes.get(call.id);
        if (!note) return { kind: "error", error: "not_found" };
        note.summary = { ...note.summary, trashed: call.kind === "trash" };
        changed(call.id);
        return { kind: "applied", request_id: "", summary: note.summary };
      }
      case "delete": {
        const note = notes.get(call.id);
        if (!note) return { kind: "error", error: "not_found" };
        if (!note.summary.trashed) return { kind: "error", error: "invalid" };
        notes.delete(call.id);
        changed(call.id);
        return { kind: "done" };
      }
      case "resolve":
        return {
          kind: "targets",
          items: call.targets.map((target) => ({
            target,
            note:
              [...notes.values()].find(
                (note) =>
                  !note.summary.trashed &&
                  note.summary.title.toLowerCase() === target.toLowerCase(),
              )?.summary ?? null,
          })),
        };
      case "backlinks":
        return { kind: "page", items: [], next: null };
      case "reveal":
        return { kind: "done" };
    }
  }

  return {
    notes,
    calls,
    call: async (expected: string, call: NoteCall): Promise<NoteReply> => {
      calls.push(structuredClone(call));
      if (expected !== profile)
        return { profile: null, response: { kind: "error", error: "unavailable" } };
      return { profile, response: respond(call) };
    },
    seed(markdown: string, fields: Partial<NoteSummary> = {}): NoteSummary {
      return store(undefined, markdown, fields).summary;
    },
    /** Another app changes a note's file. */
    editOnDisk(id: string, markdown: string): void {
      store(notes.get(id), markdown);
      changed(id, false);
    },
    removeOnDisk(id: string): void {
      notes.delete(id);
      changed(id);
    },
    writes: () => calls.filter((call) => call.kind === "write" || call.kind === "create"),
  };
}
