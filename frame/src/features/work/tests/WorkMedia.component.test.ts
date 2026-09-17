import "$styles/global.css";
import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { page } from "vitest/browser";
import MediaCard from "../components/cards/MediaCard.svelte";
import WorkMediaPicker from "../components/WorkMediaPicker.svelte";
import type { CanvasItem } from "../lib/canvas-model";
import type { MediaAssetV1 } from "$domain/resources";

const native = vi.hoisted(() => ({ resource: vi.fn(), mediaImport: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ resourceCall: native.resource, mediaImport: native.mediaImport });
});

const profile = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const digest = "c".repeat(64);
const image: MediaAssetV1 = {
  version: 1,
  kind: "image",
  mime: "image/png",
  bytes: 2048,
  digest,
  name: "keyboard.png",
  origin: { kind: "imported" },
  width: 64,
  height: 48,
};

test("an image card previews through the media route and a file card shows its shape", async () => {
  await page.viewport(900, 600);
  const item: CanvasItem = {
    id: "m1",
    title: "keyboard.png",
    kind: "Image",
    type: "media",
    detail: "image/png",
    status: "",
    media: { profile, asset: image },
  };
  const screen = await render(MediaCard, { item, selected: false });
  const img = screen.container.querySelector("img")!;
  expect(img.getAttribute("src")).toMatch(/zephium-media(:\/\/localhost|\.localhost)\//);
  expect(img.getAttribute("src")).toContain(`${profile}/${digest}`);
  expect(img.getAttribute("alt")).toBe("keyboard.png");
  await expect.element(screen.getByText("2 KB")).toBeVisible();
  await screen.unmount();
  const file = await render(MediaCard, {
    item: {
      ...item,
      title: "spec.pdf",
      kind: "PDF",
      media: {
        profile,
        asset: { ...image, kind: "pdf", mime: "application/pdf", name: "spec.pdf" },
      },
    },
    selected: false,
  });
  expect(file.container.querySelector("img")).toBeNull();
  await expect.element(file.getByText("application/pdf")).toBeVisible();
  await file.unmount();
});

test("the media tool lists the profile's media, imports through the native dialog, and attaches the result", async () => {
  await page.viewport(900, 600);
  native.resource.mockImplementation(async (_profile: string, call: { kind: string }) => ({
    profile,
    response:
      call.kind === "list"
        ? {
            kind: "page",
            items: [
              {
                id: "01ARZ3NDEKTSV4RRFFQ69G5FA1",
                revision: "1",
                title: "existing.png",
                pinned: false,
                updated_at: "0",
                completed: null,
                due_date: null,
              },
            ],
            next: null,
          }
        : { kind: "acknowledged" },
  }));
  native.mediaImport.mockResolvedValue({
    status: "ok",
    data: {
      kind: "imported",
      record: {
        id: "01ARZ3NDEKTSV4RRFFQ69G5FA2",
        revision: "1",
        created_at: "0",
        updated_at: "0",
        trashed: false,
        draft: {
          title: "new.png",
          pinned: false,
          content: { kind: "media", asset: image },
          related: [],
        },
      },
    },
  });
  const onattach = vi.fn();
  const screen = await render(WorkMediaPicker, {
    profile,
    host: "environment:space",
    kind: "image",
    attachedIds: [],
    pending: false,
    onattach,
  });
  await expect.element(screen.getByText("existing.png")).toBeVisible();
  await screen.getByRole("checkbox").click();
  await screen.getByRole("button", { name: "Add to Work (1)" }).click();
  expect(onattach).toHaveBeenLastCalledWith(["01ARZ3NDEKTSV4RRFFQ69G5FA1"]);
  await screen.getByRole("button", { name: "Import file…" }).click();
  await expect.poll(() => onattach.mock.calls.length).toBe(2);
  expect(native.mediaImport).toHaveBeenCalledExactlyOnceWith(profile);
  expect(onattach).toHaveBeenLastCalledWith(["01ARZ3NDEKTSV4RRFFQ69G5FA2"]);
  await screen.unmount();
});
