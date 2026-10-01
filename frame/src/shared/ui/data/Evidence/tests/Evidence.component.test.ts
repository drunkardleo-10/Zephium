import { expect, test, vi } from "vitest";
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

test("provider citations preserve attribution and open only through an explicit callback", async () => {
  const onopen = vi.fn();
  const evidence: EvidenceView = {
    state: "ready",
    title: "Source 1",
    origin: "example.com",
    role: "citation",
    text: "Provider supplied excerpt",
    truncated: false,
    sourceBytes: "25",
    citation: {
      provider: "OpenAI",
      model: "search-model",
      title: "Original title",
      url: "https://example.com/source",
    },
  };
  const screen = await render(Evidence, { evidence, onopen });
  expect(onopen).not.toHaveBeenCalled();
  await expect
    .element(screen.getByText("Citation supplied by OpenAI · search-model", { exact: true }))
    .toBeVisible();
  expect(screen.container.textContent).toContain("not a captured browser-page excerpt");
  await screen.getByRole("button", { name: "Open source in Browse", exact: true }).click();
  expect(onopen).toHaveBeenCalledExactlyOnceWith("https://example.com/source");
  await screen.rerender({
    evidence: { ...evidence, citation: { ...evidence.citation!, url: "javascript:alert(1)" } },
    onopen,
  });
  await expect
    .element(screen.getByRole("button", { name: "Open source in Browse", exact: true }))
    .not.toBeInTheDocument();
});
