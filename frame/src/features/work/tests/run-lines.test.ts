import { expect, test } from "vitest";
import { fanIn, fanOut, roundedPath } from "../lib/run/lines";
import { registrableSite, siteKey, siteName } from "../lib/run/site";

const segments = (routes: { x: number; y: number }[][]) =>
  routes.flatMap((route) => route.slice(1).map((point, index) => [route[index]!, point] as const));

test("a bus leaves one point once: the first line straight, each later one off the trunk", () => {
  const routes = fanOut({ x: 326, y: 32 }, 344, [
    { x: 362, y: 32 },
    { x: 362, y: 200 },
    { x: 362, y: 360 },
  ]);
  expect(routes[0]).toEqual([
    { x: 326, y: 32 },
    { x: 362, y: 32 },
  ]);
  expect(routes[1]![0]).toEqual({ x: 332, y: 32 });
  expect(routes[2]![0]).toEqual({ x: 344, y: 188 });
  // No two lines draw the same stretch of trunk: they meet only where one's elbow turns away.
  const trunk = segments(routes).filter(([a, b]) => a.x === 344 && b.x === 344);
  for (const [index, [a, b]] of trunk.entries())
    for (const [c, d] of trunk.slice(index + 1)) {
      const low = Math.max(Math.min(a.y, b.y), Math.min(c.y, d.y));
      const high = Math.min(Math.max(a.y, b.y), Math.max(c.y, d.y));
      expect(high - low).toBeLessThanOrEqual(12);
    }
});

test("lines into one end merge on a collector, each joining the one above it", () => {
  const routes = fanIn(
    [
      { x: 900, y: 32 },
      { x: 700, y: 200 },
    ],
    1000,
    { x: 1018, y: 32 },
  );
  expect(routes[0]).toEqual([
    { x: 900, y: 32 },
    { x: 1018, y: 32 },
  ]);
  expect(routes[1]!.at(0)).toEqual({ x: 700, y: 200 });
  expect(routes[1]!.at(-1)).toEqual({ x: 1012, y: 32 });
});

test("elbows turn on a 12 px radius", () => {
  expect(
    roundedPath([
      { x: 0, y: 0 },
      { x: 100, y: 0 },
      { x: 100, y: 100 },
    ]),
  ).toBe("M 0,0 L 88,0 Q 100,0 100,12 L 100,100");
});

test("a site's parts across its countries are one, named as people call it", () => {
  expect(registrableSite("www.airbnb.co.uk")).toBe("airbnb.co.uk");
  expect(registrableSite("jobs.paessler.com")).toBe("paessler.com");
  expect(siteKey("airbnb.co.uk")).toBe(siteKey("www.airbnb.com"));
  expect(siteName("ycombinator.com", ["Apply to YC | Y Combinator"])).toBe("Y Combinator");
  expect(siteName("careers.epam.com", ["Senior Rust Engineer | Remote Work With EPAM"])).toBe(
    "EPAM",
  );
  expect(siteName("cmu.edu")).toBe("Carnegie Mellon");
  // Without a real name, the address as written: never a name made from a fragment.
  expect(siteName("dthain.github.io")).toBe("dthain.github.io");
  expect(siteName("www.cl.cam.ac.uk")).toBe("cam.ac.uk");
  expect(siteName("craftinginterpreters.com", ["Scanning · Crafting Interpreters"])).toBe(
    "Crafting Interpreters",
  );
  expect(siteName("rublon.com", ["Careers – Rublon"])).toBe("Rublon");
});
