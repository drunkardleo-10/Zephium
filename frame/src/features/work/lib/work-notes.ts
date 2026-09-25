import { SvelteMap, SvelteSet } from "svelte/reactivity";
import type { NoteSession } from "$domain/notes";
import type { ArtifactView, DocumentNodeView } from "$shared/ui/data/Artifact";
import type { ResultReference } from "./project-environment-results";

/** One note per result, whether its card is saved on the canvas or not. */
export const resultKey = (reference: ResultReference) =>
  `${reference.objective}:${reference.execution}:${reference.artifact}`;

type Mark = NonNullable<DocumentNodeView["marks"]>[number];

/** Characters that would read as Markdown syntax inside a line of prose. */
const escape = (text: string) => text.replace(/[\\`*_[\]<]/gu, "\\$&");
/** A line that would open a block where it should stay a paragraph. */
const lineStart = (text: string) =>
  text.replace(/^(\s*)([#>+-]|\d+[.)])(?=\s|$)/u, (_all, space: string, mark: string) =>
    mark.length > 1 ? `${space}${mark.slice(0, -1)}\\${mark.at(-1)}` : `${space}\\${mark}`,
  );

function wrap(text: string, marks: readonly Mark[]): string {
  const has = (type: string) => marks.some((mark) => mark.type === type);
  let out = has("code") ? `\`${text.replaceAll("`", "")}\`` : escape(text);
  if (has("italic")) out = `*${out}*`;
  if (has("bold")) out = `**${out}**`;
  const href = marks.find((mark) => mark.type === "link")?.attrs?.href;
  return href ? `[${out}](${href.replaceAll(")", "%29")})` : out;
}

function inline(nodes: readonly DocumentNodeView[]): string {
  return nodes
    .map((node) =>
      node.type === "text"
        ? wrap(node.text ?? "", node.marks ?? [])
        : node.type === "hardBreak"
          ? "\\\n"
          : "",
    )
    .join("");
}

const indent = (text: string, by: string) =>
  text
    .split("\n")
    .map((line, index) => (index && line ? by + line : line))
    .join("\n");

function block(node: DocumentNodeView): string {
  const children = node.content ?? [];
  switch (node.type) {
    case "paragraph":
      return lineStart(inline(children));
    case "heading":
      return `${"#".repeat(Math.min(6, Math.max(1, node.attrs?.level ?? 2)))} ${inline(children)}`;
    case "codeBlock":
      return `\`\`\`\n${children.map((child) => child.text ?? "").join("")}\n\`\`\``;
    case "blockquote":
      return blocks(children)
        .split("\n")
        .map((line) => (line ? `> ${line}` : ">"))
        .join("\n");
    case "bulletList":
    case "orderedList": {
      const start = node.attrs?.start ?? 1;
      return children
        .map((item, index) => {
          const marker = node.type === "bulletList" ? "-" : `${start + index}.`;
          return `${marker} ${indent(blocks(item.content ?? []), " ".repeat(marker.length + 1))}`;
        })
        .join("\n");
    }
    default:
      return blocks(children);
  }
}

const blocks = (nodes: readonly DocumentNodeView[]) =>
  nodes
    .map(block)
    .filter((text) => text.trim())
    .join("\n\n");

/**
 * A document result as the person's note: its title as the heading, then the
 * formatted document, or its paragraphs as written when it has no formatting.
 */
export function documentMarkdown(view: ArtifactView): string | null {
  if (view.content.kind !== "document") return null;
  const title = view.title.replace(/\s+/gu, " ").trim();
  let nodes = view.content.formatted?.document.content ?? null;
  // A document that opens on its own title says it once.
  const first = nodes?.[0];
  if (first?.type === "heading" && inline(first.content ?? []).trim() === escape(title))
    nodes = nodes!.slice(1);
  const body = nodes
    ? blocks(nodes)
    : view.content.paragraphs
        .map((paragraph) => paragraph.trim())
        .filter(Boolean)
        .join("\n\n");
  const text = [title ? `# ${escape(title)}` : "", body].filter(Boolean).join("\n\n");
  return text ? `${text}\n` : null;
}

/**
 * The notes written from results in this canvas, while it is open. The note
 * itself lives in Notes; this only remembers which one a result became.
 */
export class WorkNotes {
  #notes = new SvelteMap<string, string>();
  #saving = new SvelteSet<string>();
  #failed = new SvelteSet<string>();

  note(key: string): string | undefined {
    return this.#notes.get(key);
  }

  saving(key: string): boolean {
    return this.#saving.has(key);
  }

  get failed(): boolean {
    return this.#failed.size > 0;
  }

  dismiss() {
    this.#failed.clear();
  }

  /** Writes the Markdown as a new note; only when asked, and once per result. */
  async save(key: string, markdown: string, session: NoteSession | null): Promise<string | null> {
    const known = this.#notes.get(key);
    if (known || this.#saving.has(key)) return known ?? null;
    this.#saving.add(key);
    this.#failed.delete(key);
    try {
      const open = session?.note;
      // A save that did not land is still the open note: finish it, never a second copy.
      if (open && !open.id && session!.markdown === markdown) await session!.flush();
      else await session?.create(markdown);
      const id = session?.markdown === markdown ? session.note?.id : null;
      if (id) this.#notes.set(key, id);
      else this.#failed.add(key);
      return id ?? null;
    } finally {
      this.#saving.delete(key);
    }
  }
}
