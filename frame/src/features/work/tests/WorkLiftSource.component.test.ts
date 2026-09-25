import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkEnvironmentSession } from "$domain/work-environment";
import WorkEnvironmentWorkspace from "../components/WorkEnvironmentWorkspace.svelte";

const native = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workCall: native.call });
});

test("the lift grows from the card it was opened on, even with another card on the same spot", async () => {
  const { workSession } = await import("$domain/work");
  const { projection, snapshot } = await import("./environment-fixtures");
  const environment = new WorkEnvironmentSession(snapshot.profile, snapshot.space);
  environment.snapshot = structuredClone(snapshot);
  environment.selected = snapshot.id;
  environment.tabsIntroduced = true;
  const objective = workSession(snapshot.profile)!;
  vi.spyOn(objective, "start").mockResolvedValue();
  vi.spyOn(objective, "open").mockImplementation(async () => {
    objective.projection = structuredClone(projection);
    return true;
  });
  vi.spyOn(objective, "plan").mockResolvedValue(null);
  native.call.mockResolvedValue({
    version: 1,
    profile: snapshot.profile,
    reply: { kind: "error", error: "not_found" },
  });
  const screen = await render(WorkEnvironmentWorkspace, {
    session: environment,
    tabs: [],
    spaceName: "Personal",
    profileLabel: "Reader",
    aiEnabled: false,
    onreturn: vi.fn(),
    onopen: vi.fn(),
    onnewtab: vi.fn(),
    onsettings: vi.fn(),
  });
  const root = screen.container.querySelector(".environment") as HTMLElement;
  root.style.height = "720px";
  root.style.width = "1100px";
  const card = () => screen.container.querySelector<HTMLElement>('[data-card-id="objective-card"]');
  await expect.poll(() => card()).not.toBeNull();
  // A card drawn exactly over it, earlier in the document: only the id tells them apart.
  const rect = card()!.getBoundingClientRect();
  const decoy = document.createElement("article");
  decoy.dataset.cardId = "decoy";
  Object.assign(decoy.style, {
    position: "fixed",
    left: `${rect.left}px`,
    top: `${rect.top}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
    pointerEvents: "none",
  });
  document.body.prepend(decoy);
  card()!.click();
  await screen.getByRole("button", { name: "Open", exact: true }).click();
  await expect.element(screen.getByRole("dialog", { name: "Request", exact: true })).toBeVisible();
  await expect.poll(() => card()!.style.visibility).toBe("hidden");
  expect(decoy.style.visibility).toBe("");
  await screen.unmount();
  decoy.remove();
  environment.dispose();
  objective.dispose();
});
