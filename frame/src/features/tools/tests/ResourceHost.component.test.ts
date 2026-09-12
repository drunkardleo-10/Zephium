import { afterEach, expect, test, vi } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import { resourceTestServer } from "$shared/testing/resources/server";
import primitives from "../../../styles/tokens/primitive.css?raw";
import tokens from "../../../styles/tokens.css?raw";
import browserCSS from "../../../styles/browser.css?raw";
import ResourceHost from "./ResourceHost.svelte";
const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ resourceCall: native.call });
});
const style = document.createElement("style");
style.textContent = primitives + tokens.replace("@theme static", ":root") + browserCSS;
afterEach(() => style.remove());
for (const host of ["sidebar", "floating"] as const) {
  for (const tool of ["notes", "tasks"] as const) {
    test(`${tool} fits the ${host} ToolSlot through list, editor and back navigation`, async () => {
      await page.viewport(1000, 800);
      document.head.append(style);
      const profile = "00000000000000000000000003";
      const server = resourceTestServer(profile);
      native.call.mockImplementation(server.call);
      const onback = vi.fn();
      const screen = await render(ResourceHost, { profile, host, tool, onback });
      await expect
        .element(
          screen.getByRole("region", { name: tool === "notes" ? "Notes" : "Tasks", exact: true }),
        )
        .toBeVisible();
      const fits = () => {
        const panel = screen.container.querySelector<HTMLElement>(".resource-panel");
        if (!panel) return false;
        const parent = panel.parentElement!;
        const box = panel.getBoundingClientRect();
        const bounds = parent.getBoundingClientRect();
        const contentFits = [
          ...panel.querySelectorAll<HTMLElement>(".resource-list, .resource-editor, .note-toolbar"),
        ].every((element) => {
          if (!element.getClientRects().length) return true;
          const child = element.getBoundingClientRect();
          return (
            child.left >= box.left - 1 &&
            child.right <= box.right + 1 &&
            element.scrollWidth <= element.clientWidth + 1
          );
        });
        return (
          contentFits &&
          box.width > 300 &&
          box.left >= bounds.left &&
          box.right <= bounds.right + 1 &&
          panel.scrollWidth <= panel.clientWidth + 1
        );
      };
      await expect.poll(fits).toBe(true);
      await screen.getByRole("button", { name: "New", exact: true }).click();
      await screen
        .getByRole("textbox", { name: "Title", exact: true })
        .fill("A long title that must remain inside its own resource surface");
      await expect.poll(fits).toBe(true);
      if (host === "sidebar") {
        await screen.getByRole("button", { name: "Back to list", exact: true }).click();
        await expect.poll(() => screen.container.querySelector(".resource-editor")).toBeNull();
      } else {
        await screen.getByRole("button", { name: "Back to search", exact: true }).click();
        expect(onback).toHaveBeenCalledOnce();
      }
      expect([...server.records.values()][0]?.draft.title).toContain("A long title");
    });
  }
}
