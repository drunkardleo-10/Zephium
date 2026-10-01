import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import CodeBlock from "../CodeBlock.svelte";

const text = 'fn main() {\n    let x = 1; // one\n    println!("{x}");\n}';
const notes = [{ from: 2, to: 3, text: "Binds and prints x" }];

test("the card numbers lines, bars noted ranges and shows the note on hover", async () => {
  const screen = await render(CodeBlock, {
    language: "rust",
    text,
    notes,
    label: "main.rs",
    limit: 3,
  });
  const rows = screen.container.querySelectorAll(".row");
  expect(rows).toHaveLength(3);
  expect([...rows].map((row) => row.querySelector(".n")?.textContent)).toEqual(["1", "2", "3"]);
  expect([...rows].map((row) => row.classList.contains("noted"))).toEqual([false, true, true]);
  expect(screen.container.querySelector(".keyword")?.textContent).toBe("fn");
  expect(screen.container.querySelector(".comment")?.textContent).toBe("// one");
  expect(screen.container.querySelector("[role=note]")).toBeNull();
  await screen.getByText("println!").hover();
  await expect.element(screen.getByRole("note")).toHaveTextContent("Binds and prints x");
  expect(screen.container.querySelector(".rail")).toBeNull();
});

test("the lift lists notes as a rail and lights the range", async () => {
  const screen = await render(CodeBlock, {
    language: "rust",
    text,
    notes,
    label: "main.rs",
    variant: "lift",
  });
  expect(screen.container.querySelectorAll(".row")).toHaveLength(4);
  const entry = screen.getByRole("button", { name: /Lines 2–3/u });
  await entry.click();
  await expect.element(entry).toHaveAttribute("aria-pressed", "true");
  expect(screen.container.querySelectorAll(".row.lit")).toHaveLength(2);
  await screen.getByText("main").hover();
  expect(screen.container.querySelector("[role=note]")).toBeNull();
});
