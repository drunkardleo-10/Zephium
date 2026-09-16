import { afterEach, describe, expect, it, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import primitives from "../../../styles/tokens/primitive.css?raw";
import tokens from "../../../styles/tokens.css?raw";
import globals from "../../../styles/global.css?raw";
import { favicons } from "..";
import { emitNativeEvent } from "$shared/testing/native-events";
import IconStrip from "./IconStrip.svelte";

const BYTE_LENGTH = 32 * 32 * 4;

function solid(red: number, green: number, blue: number): string {
  let binary = "";
  for (let index = 0; index < BYTE_LENGTH; index += 4) {
    binary += String.fromCharCode(red, green, blue, 255);
  }
  return btoa(binary);
}

const style = document.createElement("style");
style.textContent = primitives + tokens.replace("@theme static", ":root") + globals;
document.head.append(style);

const SIZE = 17;
const ORIGINS = {
  neutralDark: "https://neutral-dark.example",
  neutralLight: "https://neutral-light.example",
  coloured: "https://coloured.example",
};

async function strip() {
  await favicons.init();
  emitNativeEvent("favicons", {
    surface: "chrome",
    profile_id: "p",
    entries: [
      { origin: ORIGINS.neutralDark, revision: "a", rgba: solid(20, 21, 22) },
      { origin: ORIGINS.neutralLight, revision: "a", rgba: solid(238, 238, 238) },
      { origin: ORIGINS.coloured, revision: "a", rgba: solid(255, 0, 0) },
    ],
  });
  render(IconStrip, { origins: Object.values(ORIGINS), size: SIZE });
  await vi.waitFor(() =>
    expect(document.querySelectorAll("canvas")).toHaveLength(Object.keys(ORIGINS).length),
  );
}

function boxes(origin: string) {
  const host = document.querySelector<HTMLElement>(`[data-origin="${origin}"]`)!;
  const plate = host.querySelector<HTMLElement>(".favicon-plate")!;
  const canvas = host.querySelector<HTMLCanvasElement>("canvas")!;
  return { plate: plate.getBoundingClientRect(), canvas: canvas.getBoundingClientRect() };
}

afterEach(() => {
  favicons.dispose();
  delete document.documentElement.dataset.theme;
});

describe("plated icons", () => {
  it("insets only the marks this theme actually plates", async () => {
    await strip();

    // Dark theme: the dark neutral mark gets a ground and sits inside it.
    const dark = boxes(ORIGINS.neutralDark);
    expect(dark.plate.width).toBeCloseTo(SIZE, 0);
    expect(dark.canvas.width).toBeLessThan(dark.plate.width);

    // A light mark reads fine on dark chrome, so it is never inset: insetting
    // by tone alone shrank marks that were never plated.
    for (const origin of [ORIGINS.neutralLight, ORIGINS.coloured]) {
      const box = boxes(origin);
      expect(box.plate.width).toBeCloseTo(SIZE, 0);
      expect(box.canvas.width).toBeCloseTo(SIZE, 0);
    }
  });

  it("reverses which mark is inset in the light theme", async () => {
    document.documentElement.dataset.theme = "light";
    await strip();

    const light = boxes(ORIGINS.neutralLight);
    expect(light.canvas.width).toBeLessThan(light.plate.width);

    for (const origin of [ORIGINS.neutralDark, ORIGINS.coloured]) {
      expect(boxes(origin).canvas.width).toBeCloseTo(SIZE, 0);
    }
  });
});
