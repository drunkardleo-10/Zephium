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
