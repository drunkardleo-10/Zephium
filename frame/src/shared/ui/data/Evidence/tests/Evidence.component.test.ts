import { expect, test } from "vitest";
import { render } from "vitest-browser-svelte";
import Evidence, { type EvidenceView } from "../index";
test("presents source history as inert text with truthful truncation and byte count", async () => {
  const evidence: EvidenceView = {
    state: "ready",
    title: "Source",
    origin: "https://example.com",
    role: "Documentation",
    text: "<img src=x onerror=unsafe()>",
    truncated: true,
    sourceBytes: "9007199254740993",
  };
  const screen = await render(Evidence, { evidence });
  expect(screen.container.querySelector("img, a")).toBeNull();
  expect(screen.container.textContent).toContain("9007199254740993");
  expect(screen.container.textContent).toContain("Truncated excerpt");
  await screen.rerender({ evidence: { ...evidence, text: "é".repeat(5000) } });
  await expect.element(screen.getByRole("alert")).toBeVisible();
});
