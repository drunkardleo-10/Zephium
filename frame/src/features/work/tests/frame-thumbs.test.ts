import { expect, test } from "vitest";
import { thumbSrc } from "../lib/frame-thumbs";

test("only pictures of the media route are asked for at a width", () => {
  const media = "zephium-media://localhost/01M3CTWDG20GFZPACD7905RSRJ/" + "b".repeat(64);
  expect(thumbSrc(media, 224)).toBe(`${media}?w=224`);
  expect(thumbSrc(`${media}?v=2`, 224)).toBe(`${media}?v=2&w=224`);
  expect(thumbSrc("http://zephium-media.localhost/frame/a/b/1", 288)).toBe(
    "http://zephium-media.localhost/frame/a/b/1?w=288",
  );
  expect(thumbSrc("data:image/png;base64,AAAA", 224)).toBe("data:image/png;base64,AAAA");
  expect(thumbSrc("https://example.com/a.png", 224)).toBe("https://example.com/a.png");
});
