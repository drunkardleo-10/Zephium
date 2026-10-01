import "$styles/global.css";
import { test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import StartStage from "./StartStage.svelte";

vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({});
});

const shots = "../../../../../target/work-start";
const settle = () => new Promise((done) => setTimeout(done, 500));

test("a new work's start screen, at rest and with a workflow picked, in both themes", async () => {
  await page.viewport(1376, 884);
  render(StartStage, {
    works: [
      { requests: ["Research how competitors price issue trackers"], touched_ms: "300" },
      { requests: ["Plan my day"], touched_ms: "200" },
      {
        requests: ["Explain how this code works: ~/Dev/zephium/crates/zephium-app"],
        touched_ms: "100",
      },
    ],
  });
  const shoot = async (name: string) => {
    for (const theme of ["dark", "light"]) {
      document.documentElement.dataset.theme = theme;
      await settle();
      await page.screenshot({ path: `${shots}/${name}-${theme}.png` });
    }
  };
  await shoot("rest");
  await page.getByRole("radio", { name: "Developer" }).click();
  await page.getByRole("button", { name: /^Explain code How/u }).click();
  await shoot("picked");
});
