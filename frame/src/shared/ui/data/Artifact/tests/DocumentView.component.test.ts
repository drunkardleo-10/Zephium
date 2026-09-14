import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import DocumentView from "../DocumentView.svelte";
import type { NoteDocumentView as NoteDocument } from "../artifact";

const document: NoteDocument = {
  version: 1,
  document: {
    type: "doc",
    content: [
      { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "Summary" }] },
      {
        type: "paragraph",
        content: [
          { type: "text", text: "See " },
          {
            type: "text",
            text: "the guide",
            marks: [
              { type: "bold" },
              { type: "link", attrs: { href: "https://docs.example/guide" } },
            ],
          },
          { type: "text", text: " and ", marks: [] },
          { type: "text", text: "code", marks: [{ type: "code" }] },
        ],
      },
      {
        type: "orderedList",
        attrs: { start: 1 },
        content: [
          {
            type: "listItem",
            content: [{ type: "paragraph", content: [{ type: "text", text: "First" }] }],
          },
        ],
      },
      {
        type: "blockquote",
        content: [{ type: "paragraph", content: [{ type: "text", text: "Quoted" }] }],
      },
    ],
  },
};

test("a formatted document renders blocks natively and hands links to the host", async () => {
  const onlink = vi.fn();
  const screen = await render(DocumentView, { document, onlink });
  await expect.element(screen.getByRole("heading", { level: 4, name: "Summary" })).toBeVisible();
  await expect.element(screen.getByRole("listitem")).toHaveTextContent("First");
  await expect.element(screen.getByText("Quoted")).toBeVisible();
  expect(screen.container.querySelector("a")).toBeNull();
  const link = screen.getByRole("button", { name: "the guide" });
  await expect.element(link).toHaveAttribute("title", "https://docs.example/guide");
  await link.click();
  expect(onlink).toHaveBeenCalledExactlyOnceWith("https://docs.example/guide");
  expect(screen.container.querySelector("code")?.textContent).toBe("code");
  await screen.unmount();
});
