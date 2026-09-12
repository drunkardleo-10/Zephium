import type { JSONContent } from "@tiptap/core";
import type { DocumentNode, NoteDocument } from "$domain/resources";
/** Strip editor-only attributes. Rust independently validates this constrained schema. */
export function noteDocument(node: JSONContent): NoteDocument {
  function convert(value: JSONContent): DocumentNode {
    const kind = value.type ?? "paragraph";
    return {
      type: kind,
      ...(value.text !== undefined ? { text: value.text } : {}),
      ...(value.content?.length ? { content: value.content.map(convert) } : {}),
      ...(value.marks?.length ? { marks: value.marks.map((mark) => ({ type: mark.type })) } : {}),
      ...(kind === "heading"
        ? { attrs: { level: value.attrs?.level } }
        : kind === "noteReference"
          ? { attrs: { resource: value.attrs?.resource } }
          : kind === "orderedList"
            ? { attrs: { start: value.attrs?.start ?? 1 } }
            : {}),
    };
  }
  return { version: 1, document: convert(node) };
}
