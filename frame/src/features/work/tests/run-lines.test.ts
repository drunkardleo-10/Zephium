import { expect, test } from "vitest";
import { branch, merge, roundedPath } from "../lib/run/lines";
import { registrableSite, siteKey, siteName } from "../lib/run/site";

test("parallel parts branch from one point, up and down alike", () => {
  const start = { x: 326, y: 200 };
  const routes = branch(start, [
    { x: 410, y: 32 },
    { x: 410, y: 200 },
    { x: 410, y: 368 },
  ]);
  for (const route of routes) expect(route[0]).toEqual(start);
  expect(routes[1]).toEqual([start, { x: 410, y: 200 }]);
  // A branch leaves and lands level: a curve whose handles lie on its two ends' lines.
  expect(roundedPath(routes[0]!)).toBe("M 326,200 C 368,200 368,32 410,32");
  expect(roundedPath(routes[2]!)).toBe("M 326,200 C 368,200 368,368 410,368");
});

test("lines into one end run level to the collector, then curve in", () => {
  const routes = merge(
    [
      { x: 900, y: 32 },
      { x: 700, y: 200 },
    ],
    900,
    { x: 1018, y: 116 },
  );
  expect(routes[0]).toEqual([
    { x: 900, y: 32 },
    { x: 1018, y: 116 },
  ]);
  expect(routes[1]).toEqual([
    { x: 700, y: 200 },
    { x: 900, y: 200 },
    { x: 1018, y: 116 },
  ]);
  expect(roundedPath(routes[1]!)).toBe("M 700,200 L 900,200 C 959,200 959,116 1018,116");
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
  expect(siteName("www.justonecookbook.com", ["Rice Bowl (Video) • Just One Cookbook"])).toBe(
    "Just One Cookbook",
  );
});
