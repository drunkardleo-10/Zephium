import { expect, test } from "vitest";
import { blank, outline } from "../outline";

test("the leading heading is the title and the text after it the preview", () => {
  expect(outline("# Trip *planning*\n\nBook the [train](https://x).\n- [ ] pack")).toEqual({
    heading: "Trip planning",
    preview: "Book the train. pack",
  });
  expect(outline("\n\nNo heading here\n## Later")).toEqual({
    heading: null,
    preview: "No heading here Later",
  });
  expect(outline("---\ntags: a\n---\n# Front\nbody").heading).toBe("Front");
});

test("previews stop reading once they are long enough", () => {
  const long = `# T\n\n${"word ".repeat(100_000)}`;
  const started = performance.now();
  const { preview } = outline(long);
  expect(performance.now() - started).toBeLessThan(20);
  expect([...preview].length).toBeLessThanOrEqual(181);
  expect(preview.endsWith("…")).toBe(true);
});

test("a title with nothing under it is still blank", () => {
  expect(blank("#\n\n  ")).toBe(true);
  expect(blank("# A")).toBe(false);
});
