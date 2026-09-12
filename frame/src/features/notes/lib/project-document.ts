import type { Node } from "@tiptap/pm/model";
import type { DocumentNode, NoteDocument } from "$domain/resources";
import { noteDocument } from "./document";

const encoder = new TextEncoder();
const envelopeBytes = encoder.encode('{"version":1,"document":}').length;
type Projection = {
  node: DocumentNode;
  bytes: number;
  textBytes: number;
  count: number;
  depth: number;
  valid: boolean;
  references: readonly string[];
};

/** Editor-lifetime cache. Immutable PM nodes share unchanged branches across edits
 * and history; weak keys do not retain discarded history or rejected documents. */
export function documentProjector() {
  const cache = new WeakMap<Node, Projection>();
  function project(node: Node): Projection {
    const cached = cache.get(node);
    if (cached) return cached;
    // Serialize only this node's attributes/text/marks, never its descendants.
    const own = noteDocument({
      type: node.type.name,
      attrs: node.attrs,
      ...(node.isText ? { text: node.text } : {}),
      marks: node.marks.map((mark) => ({ type: mark.type.name })),
    }).document;
    const children: DocumentNode[] = [];
    const references = new Set<string>();
    if (own.type === "noteReference" && own.attrs?.resource) references.add(own.attrs.resource);
    let bytes = encoder.encode(JSON.stringify(own)).length;
    let textBytes = node.isText ? encoder.encode(node.text!).length : 0;
    let count = 1;
    let depth = 0;
    let valid =
      !node.text?.includes("\0") &&
      (node.type.name !== "orderedList" ||
        (Number.isInteger(node.attrs.start) && Math.abs(node.attrs.start) <= 1_000_000));
    node.forEach((child) => {
      const part = project(child);
      children.push(part.node);
      bytes += part.bytes;
      textBytes += part.textBytes;
      count += part.count;
      depth = Math.max(depth, part.depth + 1);
      valid &&= part.valid;
      // Retain at most one more than the admitted limit, sufficient to reject.
      for (const id of part.references) {
        if (references.size > 64) break;
        references.add(id);
      }
    });
    if (children.length) {
      own.content = children;
      bytes += ',"content":[]'.length + children.length - 1;
    }
    valid &&= count <= 4096 && depth <= 16 && textBytes <= 262144 && references.size <= 64;
    const result = {
      node: own,
      bytes,
      textBytes,
      count,
      depth,
      valid,
      references: [...references].sort(),
    };
    cache.set(node, result);
    return result;
  }
  return (
    node: Node,
  ): { document: NoteDocument; allowed: boolean; bytes: number; references: readonly string[] } => {
    const result = project(node);
    const bytes = result.bytes + envelopeBytes;
    return {
      document: { version: 1, document: result.node },
      allowed: result.valid && bytes <= 500000,
      bytes,
      references: result.references,
    };
  };
}
