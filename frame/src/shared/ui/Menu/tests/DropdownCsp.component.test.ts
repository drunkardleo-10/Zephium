import { expect, inject, test } from "vitest";
import { page, userEvent } from "vitest/browser";
import { render } from "vitest-browser-svelte";
import Host from "./DropdownCspHost.svelte";

test("packaged style policy allows dropdown selection and restores pointer input", async () => {
  const screen = await render(Host);
  const violations: string[] = [];
  const onViolation = (event: SecurityPolicyViolationEvent) => {
    if (event.effectiveDirective.startsWith("style-src")) violations.push(event.blockedURI);
  };
  document.addEventListener("securitypolicyviolation", onViolation);
  const { styleSource, inlineStyleCount } = inject("packagedStylePolicy");
  // Tauri adds a nonce source for each inline HTML style block. Exercise that
  // effective policy, not merely the pre-codegen style-src in tauri.conf.json.
  // Both privileged entry documents must support the same dropdown contract.
  const policy = document.createElement("meta");
  policy.httpEquiv = "Content-Security-Policy";
  policy.content = `style-src ${styleSource} ${Array.from(
    { length: inlineStyleCount },
    (_, index) => `'nonce-packaged-style-${index}'`,
  ).join(" ")}`;
  document.head.append(policy);
  try {
    await page.getByRole("button", { name: "Actions", exact: true }).click();
    await page.getByRole("menuitem", { name: "Choose action" }).click();
    await expect.poll(() => getComputedStyle(document.body).pointerEvents).toBe("auto");
    await page.getByRole("button", { name: "After dropdown" }).click();
    await expect.element(page.getByText("Clicks: 2", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Choice", exact: true }).click();
    await page.getByRole("option", { name: "Second", exact: true }).click();
    await expect.poll(() => getComputedStyle(document.body).pointerEvents).toBe("auto");
    await page.getByRole("button", { name: "After dropdown" }).click();
    await expect.element(page.getByText("Clicks: 3", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Actions", exact: true }).click();
    await userEvent.keyboard("{Escape}");
    await expect.poll(() => getComputedStyle(document.body).pointerEvents).toBe("auto");
    await page.getByRole("button", { name: "After dropdown" }).click();
    await expect.element(page.getByText("Clicks: 4", { exact: true })).toBeVisible();
    expect(violations).toEqual([]);
  } finally {
    document.removeEventListener("securitypolicyviolation", onViolation);
    await screen.unmount();
    policy.remove();
  }
});

declare module "vitest" {
  export interface ProvidedContext {
    packagedStylePolicy: { styleSource: string; inlineStyleCount: number };
  }
}
