import { expect, test } from "vitest";
import { youtubeId, youtubeThumbnail } from "../lib/link-media";
import { environmentItems } from "../lib/project-environment";
import type { WorkEnvironmentSnapshot } from "$shared/ipc/bindings";
import type { MediaAssetV1 } from "$domain/resources";

test("a video id comes from watch, youtu.be and shorts addresses only", () => {
  const id = "dQw4w9WgXcQ";
  expect(youtubeId(`https://www.youtube.com/watch?v=${id}&t=4`)).toBe(id);
  expect(youtubeId(`https://m.youtube.com/watch?v=${id}`)).toBe(id);
  expect(youtubeId(`https://youtu.be/${id}?si=x`)).toBe(id);
  expect(youtubeId(`https://youtube.com/shorts/${id}`)).toBe(id);
  expect(youtubeId("https://www.youtube.com/@channel")).toBeNull();
  expect(youtubeId(`https://notyoutube.com/watch?v=${id}`)).toBeNull();
  expect(youtubeId("https://youtu.be/short")).toBeNull();
  expect(youtubeThumbnail(`https://youtu.be/${id}`)).toBe(
    `https://i.ytimg.com/vi/${id}/hqdefault.jpg`,
  );
});

test("a link's admitted picture is its image, never a media card of its own", () => {
  const snapshot: WorkEnvironmentSnapshot = {
    version: 1,
    profile: "p",
    id: "env",
    space: "s",
    title: "T",
    revision: "2",
    lifecycle: "active",
    areas: [],
    view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
    elements: [
      {
        id: "link",
        area: null,
        reference: { kind: "link", url: "https://youtu.be/x", title: "v" },
      },
      { id: "picture", area: null, reference: { kind: "resource", resource: "r" } },
    ],
    relations: [{ id: "u", from: "link", to: "picture", kind: "uses", origin: { kind: "user" } }],
  };
  const asset: MediaAssetV1 = {
    version: 1,
    kind: "image",
    mime: "image/jpeg",
    bytes: 1,
    digest: "f".repeat(64),
    name: "hqdefault.jpg",
    origin: { kind: "imported" },
  };
  const items = environmentItems(snapshot, [], [], new Map(), new Map([["r", asset]]));
  expect(items.map((item) => item.id)).toEqual(["link"]);
  expect(items[0]!.image).toEqual({ profile: "p", digest: "f".repeat(64) });
});
