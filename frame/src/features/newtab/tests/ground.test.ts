import { describe, expect, it } from "vitest";
import { groundPath, notchShape, notchWidth, roundedRect, type Notch } from "../lib/ground";

const NOTCH: Notch = { width: 600, height: 64, radius: 20, flare: 12 };

const CONTOUR = [
  "A12 12 0 0 1 200 12",
  "V44",
  "A20 20 0 0 0 220 64",
  "H780",
  "A20 20 0 0 0 800 44",
  "V12",
  "A12 12 0 0 1 812 0",
].join("");

describe("groundPath", () => {
  it("centres the notch on the top edge with rounded corners and flared feet", () => {
    expect(groundPath(1000, 800, NOTCH)).toBe(`M0 0H188${CONTOUR}H1000V800H0Z`);
  });

  it("cuts each opening through the ground as its own subpath", () => {
    const opening = roundedRect(100, 300, 200, 100, 16);
    expect(groundPath(1000, 800, NOTCH, [opening])).toBe(
      `M0 0H188${CONTOUR}H1000V800H0Z${opening}`,
    );
  });

  it("narrows the notch to leave ground on either side", () => {
    expect(notchWidth(400, NOTCH)).toBe(360);
    expect(groundPath(400, 800, NOTCH)).toContain("H8A12 12 0 0 1 20 12");
  });

  it("leaves the notch out where the page has no room for it", () => {
    expect(groundPath(50, 800, NOTCH)).toBe("M0 0H50V800H0Z");
    expect(groundPath(1000, 40, NOTCH)).toBe("M0 0H1000V40H0Z");
  });

  it("draws nothing before the page has a size", () => {
    expect(groundPath(0, 0, NOTCH)).toBe("");
  });
});

describe("notchShape", () => {
  it("closes the notch along the top edge", () => {
    expect(notchShape(1000, 800, NOTCH)).toBe(`M188 0${CONTOUR}Z`);
  });

  it("is empty where the ground has no notch", () => {
    expect(notchShape(50, 800, NOTCH)).toBe("");
  });
});

describe("roundedRect", () => {
  it("rounds each corner, never past half the shorter side", () => {
    expect(roundedRect(10, 20, 100, 40, 16)).toBe(
      "M26 20H94A16 16 0 0 1 110 36V44A16 16 0 0 1 94 60H26A16 16 0 0 1 10 44V36A16 16 0 0 1 26 20Z",
    );
    expect(roundedRect(0, 0, 40, 20, 99)).toContain("A10 10");
  });
});
