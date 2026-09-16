import { describe, expect, it, afterEach } from "vitest";
import { favicons } from "..";
import { emitNativeEvent } from "$shared/testing/native-events";

const SIDE = 32;
const BYTE_LENGTH = SIDE * SIDE * 4;

function raster(fill: number): string {
  let binary = "";
  for (let index = 0; index < BYTE_LENGTH; index += 1) binary += String.fromCharCode(fill);
  return btoa(binary);
}

/** One opaque colour across the whole square. */
function solid(red: number, green: number, blue: number): string {
  let binary = "";
  for (let index = 0; index < BYTE_LENGTH; index += 4) {
    binary += String.fromCharCode(red, green, blue, 255);
  }
  return btoa(binary);
}

function deliver(entries: { origin: string; revision: string; rgba: string }[]) {
  emitNativeEvent("favicons", { surface: "chrome", profile_id: "p", entries });
}

afterEach(() => favicons.dispose());

describe("favicon rasters", () => {
  it("paints an origin once its pixels arrive", async () => {
    await favicons.init();
    const ref = { origin: "https://example.com", revision: "a" };
    expect(favicons.image(ref)).toBeNull();

    deliver([{ origin: ref.origin, revision: "a", rgba: raster(7) }]);

    const image = favicons.image(ref);
    expect(image?.width).toBe(SIDE);
    expect(image?.data[0]).toBe(7);
  });

  it("withholds pixels that belong to a superseded revision", async () => {
    await favicons.init();
    deliver([{ origin: "https://example.com", revision: "a", rgba: raster(7) }]);

    // Native published a new reference; the old pixels must not be drawn for it.
    expect(favicons.image({ origin: "https://example.com", revision: "b" })).toBeNull();

    deliver([{ origin: "https://example.com", revision: "b", rgba: raster(9) }]);
    expect(favicons.image({ origin: "https://example.com", revision: "b" })?.data[0]).toBe(9);
  });

  it("ignores a payload that is not one fixed-size raster", async () => {
    await favicons.init();
    deliver([
      { origin: "https://short.example", revision: "a", rgba: btoa("nope") },
      { origin: "https://bad.example", revision: "a", rgba: "!!!not base64!!!" },
    ]);

    expect(favicons.image({ origin: "https://short.example", revision: "a" })).toBeNull();
    expect(favicons.image({ origin: "https://bad.example", revision: "a" })).toBeNull();
  });

  it("holds at least as many origins as native tracks", async () => {
    await favicons.init();
    const first = { origin: "https://origin-0.example", revision: "a" };
    deliver(
      Array.from({ length: 512 }, (_, index) => ({
        origin: `https://origin-${index}.example`,
        revision: "a",
        rgba: raster(1),
      })),
    );

    // Native only resends what it believes this surface lacks, so evicting
    // inside its own bound would strand an origin permanently.
    expect(favicons.image(first)).not.toBeNull();
  });
});

describe("icon tone", () => {
  // Measured from real cached favicons: GitHub's mark averages luminance 22 at
  // chroma 2, while YouTube's averages luminance 69 at chroma 240. Only the
  // first is invisible on dark chrome.
  it("marks a near-neutral dark mark, and leaves a coloured one alone", async () => {
    await favicons.init();
    deliver([
      { origin: "https://neutral-dark.example", revision: "a", rgba: solid(20, 21, 22) },
      { origin: "https://coloured-dark.example", revision: "a", rgba: solid(255, 0, 0) },
      { origin: "https://neutral-light.example", revision: "a", rgba: solid(238, 238, 238) },
      { origin: "https://midtone.example", revision: "a", rgba: solid(128, 128, 128) },
    ]);

    expect(favicons.tone({ origin: "https://neutral-dark.example", revision: "a" })).toBe("dark");
    expect(favicons.tone({ origin: "https://coloured-dark.example", revision: "a" })).toBe("mid");
    expect(favicons.tone({ origin: "https://neutral-light.example", revision: "a" })).toBe("light");
    expect(favicons.tone({ origin: "https://midtone.example", revision: "a" })).toBe("mid");
  });

  it("judges the mark rather than the empty field around it", async () => {
    await favicons.init();
    // A small dark mark on a transparent square: averaging every pixel would
    // read as transparent-neutral and miss it.
    let binary = "";
    for (let index = 0; index < BYTE_LENGTH; index += 4) {
      const pixel = index / 4;
      const inside = pixel % 32 < 4 && pixel < 32 * 4;
      binary += inside ? String.fromCharCode(16, 16, 16, 255) : String.fromCharCode(0, 0, 0, 0);
    }
    deliver([{ origin: "https://sparse.example", revision: "a", rgba: btoa(binary) }]);

    expect(favicons.tone({ origin: "https://sparse.example", revision: "a" })).toBe("dark");
  });

  it("has no opinion about an origin whose pixels have not arrived", async () => {
    await favicons.init();
    expect(favicons.tone({ origin: "https://unknown.example", revision: "a" })).toBe("mid");
    expect(favicons.tone(null)).toBe("mid");
  });
});
