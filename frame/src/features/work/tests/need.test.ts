import { expect, test } from "vitest";
import { needWords } from "../lib/run/need";

test("a service the person has not connected asks once to be connected, with one action", () => {
  const need = { kind: "connect" as const, target: "Linear" };
  expect(needWords(need, "Issues", "row")).toEqual({
    text: "Connect Linear to read it directly",
    action: "Connect Linear",
  });
  expect(needWords(need, "Issues", "island").action).toBe("Connect Linear");
});
