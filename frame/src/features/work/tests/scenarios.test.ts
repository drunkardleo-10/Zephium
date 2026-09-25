import { expect, test } from "vitest";
import { surfaceRenderable } from "../lib/work-surface";
import { scenarios } from "./scenarios";
test.each(scenarios)("retains the bounded $title handoff scenario", (view) => {
  expect(surfaceRenderable(view)).toBe(true);
});
