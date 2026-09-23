import { expect, test } from "vitest";
import { orderBetween, orderKey, orderValue, placement, reorderKeys } from "../lib/task-order";

test("keys compare as text exactly as they compare as numbers", () => {
  const keys = [orderKey(1), orderKey(2), orderKey(10), orderKey(1_000_000_000)];
  expect([...keys].sort()).toEqual(keys);
  expect(keys.every((key) => key.length === 12)).toBe(true);
});

test("a key is only a key if it is one", () => {
  expect(orderValue("000000001000")).toBe(1000);
  expect(orderValue(null)).toBeNull();
  expect(orderValue("")).toBeNull();
  expect(orderValue("12a")).toBeNull();
  expect(orderValue("000000000000")).toBeNull();
});

test("an insert lands between its neighbours", () => {
  const first = orderBetween(null, null)!;
  const afterFirst = orderBetween(first, null)!;
  expect(afterFirst > first).toBe(true);
  const between = orderBetween(first, afterFirst)!;
  expect(between > first && between < afterFirst).toBe(true);
  const beforeFirst = orderBetween(null, first)!;
  expect(beforeFirst < first).toBe(true);
});

test("the gap runs out honestly rather than silently colliding", () => {
  let low = orderKey(1000);
  const high = orderKey(1002);
  // One key fits between 1000 and 1002; nothing fits between neighbours.
  low = orderBetween(low, high)!;
  expect(low).toBe(orderKey(1001));
  expect(orderBetween(orderKey(1000), orderKey(1001))).toBeNull();
  // And there is no room below the very first position.
  expect(orderBetween(null, orderKey(1))).toBeNull();
});

test("a drop reads as before or after the card it landed on", () => {
  const ids = ["a", "b", "c"];
  expect(placement(ids, "c", "a", false)).toEqual({ before: null, after: "a" });
  expect(placement(ids, "c", "a", true)).toEqual({ before: "a", after: "b" });
  expect(placement(ids, "a", "c", true)).toEqual({ before: "c", after: null });
  // Dropping on empty space, or on itself, means the end of the column.
  expect(placement(ids, "a", null, false)).toEqual({ before: "c", after: null });
  expect(placement(ids, "b", "b", false)).toEqual({ before: "c", after: null });
});

test("a move between positioned neighbours is one write", () => {
  const keys = reorderKeys(
    [
      { id: "a", sortKey: orderKey(1_000) },
      { id: "c", sortKey: orderKey(900_000) },
      { id: "b", sortKey: orderKey(2_000) },
    ],
    "c",
  );
  expect(Object.keys(keys)).toEqual(["c"]);
  expect(keys.c! > orderKey(1_000) && keys.c! < orderKey(2_000)).toBe(true);
});

test("a move among unpositioned tasks positions the group in its drawn order", () => {
  const ordered = [
    { id: "a", sortKey: null },
    { id: "c", sortKey: null },
    { id: "b", sortKey: null },
  ];
  const keys = reorderKeys(ordered, "c");
  const drawn = ordered
    .map((row) => ({ ...row, sortKey: keys[row.id] ?? row.sortKey }))
    .sort((left, right) => left.sortKey!.localeCompare(right.sortKey!))
    .map((row) => row.id);
  expect(drawn).toEqual(["a", "c", "b"]);
});

test("moving to the top ahead of unpositioned tasks needs only the mover", () => {
  expect(
    Object.keys(
      reorderKeys(
        [
          { id: "c", sortKey: null },
          { id: "a", sortKey: null },
        ],
        "c",
      ),
    ),
  ).toEqual(["c"]);
});
