import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { userEvent } from "vitest/browser";
import NotesEditor from "../components/NotesEditor.svelte";
import type { NoteDocument } from "$domain/resources";

test("typing in a large linked note preserves content and does not refetch reference titles", async () => {
  const id = "00000000000000000000000001";
  const value: NoteDocument = {
    version: 1,
    document: {
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "noteReference", attrs: { resource: id } }] },
        ...Array.from({ length: 1000 }, (_, i) => ({
          type: "paragraph",
          content: [{ type: "text", text: `Evidence ${i}: ${"context ".repeat(8)}` }],
        })),
      ],
    },
  };
  const resolveNotes = vi.fn(async () => [
    {
      id,
      title: "Source note",
      revision: "1",
      pinned: false,
      updated_at: "1",
      completed: null,
      due_date: null,
      due_time: null,
      status: null,
      assignee: null,
      origin: null,
      context: null,
      sort_key: null,
      work: null,
    },
  ]);
  const onchange = vi.fn();
  const screen = await render(NotesEditor, {
    value,
    onchange,
    onopen: vi.fn(),
    findNotes: async () => [],
    resolveNotes,
  });
  screen.container.style.width = "700px";
  await expect
    .element(screen.getByRole("button", { name: "Source note", exact: true }))
    .toBeVisible();
  const textbox = screen.getByRole("textbox", { name: "Notes", exact: true });
  await textbox.click();
  await userEvent.keyboard("{Control>}{End}{/Control}");
  await userEvent.keyboard(" Additional evidence.");
  await expect.poll(() => onchange.mock.calls.length).toBeGreaterThan(0);
  const latest = onchange.mock.calls.at(-1)![0] as NoteDocument;
  expect(latest.document.content).toHaveLength(1001);
  expect(JSON.stringify(latest)).toContain("Additional evidence.");
  expect(resolveNotes).toHaveBeenCalledOnce();
  // Parent draft updates may supply a fresh callback without changing references.
  const replacement = vi.fn(async () =>
    (await resolveNotes()).map((row) => ({ ...row, title: "Renamed source" })),
  );
  await screen.rerender({ value: latest, resolveNotes: replacement });
  await screen.getByRole("button", { name: "Bold", exact: true }).click();
  expect(replacement).not.toHaveBeenCalled();
  const edits = onchange.mock.calls.length;
  await screen.rerender({ referencesRevision: 1 });
  await expect.poll(() => replacement.mock.calls.length).toBe(1);
  await expect
    .element(screen.getByRole("button", { name: "Renamed source", exact: true }))
    .toBeVisible();
  expect(onchange.mock.calls.length).toBe(edits);
  await screen.unmount();
  expect(screen.container.querySelector(".tiptap")).toBeNull();
});
