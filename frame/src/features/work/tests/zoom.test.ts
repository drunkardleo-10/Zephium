import { expect, test } from "vitest";
import { detailAt } from "../lib/zoom";

test("detail follows the zoom and holds 3% past each edge", () => {
  expect(detailAt(1)).toBe("full");
  expect(detailAt(0.5)).toBe("overview");
  expect(detailAt(0.3)).toBe("tile");
  expect(detailAt(0.73, "full")).toBe("full");
  expect(detailAt(0.71, "full")).toBe("overview");
  expect(detailAt(0.74, "overview")).toBe("overview");
  expect(detailAt(0.75, "overview")).toBe("full");
  expect(detailAt(0.38, "overview")).toBe("overview");
  expect(detailAt(0.36, "overview")).toBe("tile");
  expect(detailAt(0.42, "tile")).toBe("tile");
  expect(detailAt(0.44, "tile")).toBe("overview");
});
