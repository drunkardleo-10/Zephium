import { expect, test } from "vitest";
import { commandId, newerRevision, validRevision } from "../work-model";

test("admits only canonical positive SQLite revisions without floating-point rounding", () => {
  for (const value of ["1", "9007199254740993", "9223372036854775807"])
    expect(validRevision(value)).toBe(true);
  for (const value of ["0", "01", "-1", "1.0", "1e3", "9223372036854775808"])
    expect(validRevision(value)).toBe(false);
  expect(newerRevision("9007199254740993", "9007199254740992")).toBe(true);
  expect(newerRevision("9007199254740992", "9007199254740993")).toBe(false);
});
test("generates fresh canonical command correlations", () => {
  const commands = Array.from({ length: 128 }, commandId);
  expect(new Set(commands).size).toBe(commands.length);
  for (const id of commands) expect(id).toMatch(/^[0-7][0-9A-HJKMNP-TV-Z]{25}$/u);
});
