import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import Artifact from "../Artifact.svelte";
import type { ArtifactView } from "../artifact";
const view = (content: ArtifactView["content"]): ArtifactView => ({
  key: "a",
  title: "Result",
  content,
  reviewLabel: "Needs review",
  evidence: [{ key: "source", label: "Source 1" }],
});
test("renders hostile document text and delegates evidence inspection without navigation", async () => {
  const onevidence = vi.fn();
  const screen = await render(Artifact, {
    artifact: view({ kind: "document", paragraphs: ["<script>alert(1)</script>"] }),
    onevidence,
  });
  expect(screen.container.querySelector("script")).toBeNull();
  expect(screen.container.textContent).toContain("<script>alert(1)</script>");
  await screen.getByRole("button", { name: "Source 1" }).click();
  expect(onevidence).toHaveBeenCalledExactlyOnceWith({ key: "source", label: "Source 1" });
});
test("does not create a clickable or decoded browser resource from untrusted URLs", async () => {
  const screen = await render(Artifact, {
    artifact: view({
      kind: "browser",
      title: "Unsafe",
      location: "javascript:alert(1)",
      summary: "Historical",
    }),
  });
  expect(screen.container.querySelector("a, img, iframe, canvas")).toBeNull();
  expect(screen.container.textContent).toContain("Location unavailable");
});
test("rejects malformed table dimensions", async () => {
  const screen = await render(Artifact, {
    artifact: view({ kind: "table", columns: ["Cost"], rows: [["12", "extra"]] }),
  });
  await expect.element(screen.getByRole("alert")).toBeVisible();
  expect(screen.container.querySelector("table")).toBeNull();
});
test("checklist facts are presented without frontend completion toggles", async () => {
  const screen = await render(Artifact, {
    artifact: view({ kind: "checklist", items: [{ text: "Review", completed: false }] }),
  });
  expect(screen.container.querySelector('input[type="checkbox"]')).toBeNull();
  await expect.element(screen.getByLabelText("Not completed")).toBeVisible();
});
