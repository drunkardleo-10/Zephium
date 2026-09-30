import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { BlockerSiteContext, PersonalHideView } from "$shared/ipc/bindings";

const context: BlockerSiteContext = {
  profile: "profile",
  tab: "tab",
  site: "example.com",
  revision: "0000000000000001",
};
let hides: PersonalHideView[] = [];

vi.mock("../blocker.svelte", () => ({
  status: () => ({ site: { context, hides } }),
}));
vi.mock("../site-actions", () => ({ picker: vi.fn(), changeSite: vi.fn() }));

const actions = await import("../site-actions");
const hiding = await import("../hiding.svelte");
const picker = vi.mocked(actions.picker);
const changeSite = vi.mocked(actions.changeSite);

beforeEach(() => {
  vi.useFakeTimers();
  hides = [{ id: "old", label: "Old", enabled: true }];
  changeSite.mockResolvedValue({ state: "processed" } as never);
});
afterEach(() => {
  hiding.finish();
  vi.useRealTimers();
  picker.mockReset();
  changeSite.mockReset();
});

const view = (session: string, selection = false) => ({
  session,
  active: true,
  selection: selection
    ? { identity: "a".repeat(64), label: "Banner", count: 1, positional: false }
    : null,
});

it("saves a pick at once, then takes the next one in a fresh session", async () => {
  picker.mockResolvedValueOnce(view("s1"));
  expect(await hiding.start(context)).toBe(true);
  picker.mockResolvedValueOnce(view("s1", true)).mockResolvedValueOnce(view("s2"));
  await vi.advanceTimersByTimeAsync(200);
  expect(changeSite).toHaveBeenCalledWith(context, {
    kind: "save_selection",
    session: "s1",
    selection: "a".repeat(64),
  });
  expect(picker).toHaveBeenLastCalledWith(context, { kind: "start" });
  picker.mockResolvedValueOnce(view("s2"));
  await vi.advanceTimersByTimeAsync(200);
  expect(picker).toHaveBeenLastCalledWith(context, { kind: "read", session: "s2" });
});

it("undo removes only hides added while hiding, newest first", async () => {
  picker.mockResolvedValue(view("s1"));
  await hiding.start(context);
  hides = [...hides, { id: "new", label: "Banner", enabled: true }];
  expect(hiding.added().map((hide) => hide.id)).toEqual(["new"]);
  await hiding.undo();
  expect(changeSite).toHaveBeenCalledWith(context, { kind: "remove_hide", id: "new" });
});

it("ends when the page stops picking and releases the session on finish", async () => {
  picker.mockResolvedValueOnce(view("s1")).mockResolvedValueOnce({ ...view("s1"), active: false });
  await hiding.start(context);
  await vi.advanceTimersByTimeAsync(200);
  expect(hiding.isActive()).toBe(false);

  picker.mockResolvedValue(view("s3"));
  await hiding.start(context);
  hiding.finish();
  expect(picker).toHaveBeenLastCalledWith(context, { kind: "stop", session: "s3" });
});
