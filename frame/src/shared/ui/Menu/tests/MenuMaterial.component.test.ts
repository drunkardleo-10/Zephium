import { expect, test } from "vitest";
import { page, userEvent } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import Host from "./MenuMaterialHost.svelte";

for (const opaque of [true, false]) {
  test(
    opaque
      ? "sidebar policy follows menus through portals"
      : "unmarked Settings and macOS menus retain blur",
    async () => {
      const root = document.documentElement;
      const previous = root.getAttribute("style");
      root.style.setProperty("--color-float", "rgb(61, 61, 68)");
      root.style.setProperty("--color-menu", "rgba(61, 61, 68, 0.66)");
      const screen = await render(Host, { opaque });
      try {
        for (const label of ["Actions", "Choice", "Utilities"]) {
          await page.getByRole("button", { name: label, exact: true }).click();
          const surface = () =>
            document.querySelector<HTMLElement>(
              '.ui-menu[data-state="open"], .ui-menu[data-open="true"]',
            );
          await expect
            .poll(() => surface()?.getAttribute("data-menu-material"))
            .toBe(opaque ? "opaque" : null);
          await expect
            .poll(() => surface() && getComputedStyle(surface()!).backdropFilter)
            .toBe(opaque ? "none" : "blur(24px)");
          await expect
            .poll(() => surface() && getComputedStyle(surface()!).backgroundColor)
            .toBe(opaque ? "rgb(61, 61, 68)" : "rgba(61, 61, 68, 0.66)");
          await userEvent.keyboard("{Escape}");
        }
      } finally {
        await screen.unmount();
        if (previous === null) root.removeAttribute("style");
        else root.setAttribute("style", previous);
      }
    },
  );
}
