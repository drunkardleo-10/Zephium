import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import { WorkEnvironmentSession } from "$domain/work-environment";
import type { WorkCallV1, WorkEnvironmentSnapshot } from "$shared/ipc/bindings";
import WorkEnvironmentWorkspace from "../components/WorkEnvironmentWorkspace.svelte";

const native = vi.hoisted(() => ({
  call: vi.fn(),
  admit: vi.fn(),
  resource: vi.fn(),
  paneShow: vi.fn(),
  settle: vi.fn(),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({
    workCall: native.call,
    mediaAdmitRemote: native.admit,
    resourceCall: native.resource,
    workPaneShow: native.paneShow,
    workPaneHide: vi.fn(),
    workPaneSetRect: vi.fn(),
  });
});
vi.mock("$domain/operations", () => ({ settle: native.settle }));

const profile = "00000000000000000000000001";
const video = "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42";

test("a video link admits its thumbnail as its picture and plays in the pane", async () => {
  await page.viewport(1200, 800);
  const snapshot: WorkEnvironmentSnapshot = {
    version: 1,
    profile,
    id: "00000000000000000000000003",
    space: "00000000000000000000000002",
    title: "Videos",
    revision: "1",
    lifecycle: "active",
    areas: [],
    view: {
      revision: "1",
      x: 0,
      y: 0,
      zoom_milli: 1000,
      placements: [{ element: "link", x: 80, y: 80, width: 248, height: 120 }],
    },
    elements: [
      { id: "link", area: null, reference: { kind: "link", url: video, title: "youtube.com" } },
    ],
  };
  const environment = new WorkEnvironmentSession(profile, snapshot.space);
  environment.snapshot = snapshot;
  environment.selected = snapshot.id;
  environment.tabsIntroduced = true;
  native.call.mockImplementation(async (_profile: string, _call: WorkCallV1) => ({
    version: 1,
    profile,
    reply: { kind: "environment", reply: { kind: "snapshot", snapshot: environment.snapshot! } },
  }));
  native.resource.mockImplementation(async (_profile: string, call: { kind: string }) => ({
    profile,
    response:
      call.kind === "get"
        ? {
            kind: "record",
            record: {
              id: "thumbnail",
              revision: "1",
              created_at: "0",
              updated_at: "0",
              trashed: false,
              draft: {
                title: "hqdefault.jpg",
                pinned: false,
                related: [],
                content: {
                  kind: "media",
                  asset: {
                    version: 1,
                    kind: "image",
                    mime: "image/jpeg",
                    bytes: 4096,
                    digest: "e".repeat(64),
                    name: "hqdefault.jpg",
                    origin: {
                      kind: "fetched",
                      url: "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
                      observed_at: "2026-09-24",
                    },
                  },
                },
              },
            },
          }
        : { kind: "page", items: [], next: null },
  }));
  native.admit.mockImplementation(
    async (_profile: string, _environment: string, element: string) => {
      const current = environment.snapshot!;
      environment.snapshot = {
        ...current,
        revision: String(BigInt(current.revision) + 1n),
        elements: [
          ...current.elements,
          { id: "picture", area: null, reference: { kind: "resource", resource: "thumbnail" } },
        ],
        relations: [
          { id: "uses", from: element, to: "picture", kind: "uses", origin: { kind: "user" } },
        ],
      };
      return { status: "ok", data: { kind: "admitted", element: "picture" } };
    },
  );
  native.paneShow.mockResolvedValue({ accepted: true, operation_id: "0000000000000001" });
  native.settle.mockResolvedValue({ outcome: "applied", disposition: null });
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
  await expect.poll(() => native.admit.mock.calls.length).toBe(1);
  expect(native.admit.mock.calls[0]!.slice(1)).toEqual([
    snapshot.id,
    "link",
    "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg",
  ]);
  // The thumbnail is the link card's picture, never a media card of its own.
  await expect
    .poll(() => screen.container.querySelector(".thumbnail .play-mark") !== null)
    .toBe(true);
  expect(screen.container.textContent).not.toContain("hqdefault.jpg");
  await screen.getByRole("button", { name: "Play here" }).click();
  await expect.poll(() => native.paneShow.mock.calls.length).toBeGreaterThan(0);
  expect(native.paneShow.mock.lastCall?.[0]).toEqual({ kind: "url", url: video });
  await screen.unmount();
  environment.dispose();
});
