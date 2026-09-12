import { expect, test } from "vitest";
import { Schema } from "@tiptap/pm/model";
import { documentProjector } from "../lib/project-document";
import { noteDocument } from "../lib/document";

const schema = new Schema({
  nodes: {
    doc: { content: "block+" },
    paragraph: { group: "block", content: "inline*" },
    blockquote: { group: "block", content: "block+" },
    orderedList: { group: "block", content: "paragraph+", attrs: { start: { default: 1 } } },
    text: { group: "inline" },
    noteReference: { group: "inline", inline: true, atom: true, attrs: { resource: {} } },
  },
  marks: { bold: {}, italic: {} },
});
const paragraph = (text: string) => schema.node("paragraph", null, schema.text(text));
const bytes = (value: unknown) => new TextEncoder().encode(JSON.stringify(value)).length;

test("cached projection preserves canonical data and exact UTF-8 wire sizing", () => {
  const doc = schema.node("doc", null, [
    paragraph('Unicode 🙂 漢字, escapes: " \\ \n'),
    schema.node("orderedList", { start: 42 }, [paragraph("Ordered")]),
    schema.node("paragraph", null, [
      schema.text("Bold", [schema.mark("bold")]),
      schema.node("noteReference", { resource: "00000000000000000000000001" }),
    ]),
  ]);
  const result = documentProjector()(doc);
  const canonical = noteDocument(doc.toJSON());
  expect(result.document).toEqual(canonical);
  expect(result.bytes).toBe(bytes(canonical));
  expect(result.references).toEqual(["00000000000000000000000001"]);
  expect(result.allowed).toBe(true);
});

test("rejects node, depth, text, wire and reference limits without truncating accepted data", () => {
  const project = documentProjector();
  expect(project(schema.node("doc", null, [paragraph("\0")])).allowed).toBe(false);
  expect(project(schema.node("doc", null, [paragraph("🙂".repeat(65537))])).allowed).toBe(false);
  expect(project(schema.node("doc", null, [paragraph('"'.repeat(250000))])).allowed).toBe(false);
  expect(
    project(
      schema.node(
        "doc",
        null,
        Array.from({ length: 2050 }, () => paragraph("x")),
      ),
    ).allowed,
  ).toBe(false);
  let nested = paragraph("Deep");
  for (let i = 0; i < 16; i++) nested = schema.node("blockquote", null, nested);
  expect(project(schema.node("doc", null, nested)).allowed).toBe(false);
  const links = Array.from({ length: 65 }, (_, i) =>
    schema.node("noteReference", { resource: String(i).padStart(26, "0") }),
  );
  expect(project(schema.node("doc", null, schema.node("paragraph", null, links))).allowed).toBe(
    false,
  );
  expect(
    project(
      schema.node("doc", null, schema.node("orderedList", { start: 1000001 }, paragraph("x"))),
    ).allowed,
  ).toBe(false);
});

test("a large note reuses unchanged canonical branches across typing and undo", () => {
  const children = Array.from({ length: 1800 }, (_, i) =>
    paragraph(`Paragraph ${i}: ${"evidence ".repeat(8)}`),
  );
  const doc = schema.node("doc", null, children);
  const project = documentProjector();
  const initial = project(doc);
  expect(initial.allowed).toBe(true);
  const start = performance.now();
  for (let i = 0; i < 100; i++) {
    const edited = schema.node("doc", null, [...children.slice(0, -1), paragraph(`Edited ${i}`)]);
    const next = project(edited);
    expect(next.document.document.content![0]).toBe(initial.document.document.content![0]);
    expect(next.document.document.content![1799]).not.toBe(
      initial.document.document.content![1799],
    );
    expect(next.allowed).toBe(true);
  }
  console.warn(
    `1800-paragraph projection, 100 edits: ${Math.round(performance.now() - start)}ms (includes assertions)`,
  );
  expect(project(doc).document.document).toBe(initial.document.document);
});
