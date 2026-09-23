import { expect, test } from "vitest";
import { parseCapture, parseTokens } from "../lib/task-language";
import { nextWeek } from "../lib/task-calendar";

// A Sunday, so weekday arithmetic is easy to read in the expectations.
const TODAY = "2026-09-20";
const read = (input: string) => parseCapture(input, TODAY);

test("reads a day off the end of the line and keeps the rest as the title", () => {
  expect(read("Call Anna tomorrow")).toMatchObject({
    title: "Call Anna",
    dueDate: "2026-09-21",
    dueTime: null,
    matched: "tomorrow",
  });
  expect(read("Send the invoice today")).toMatchObject({
    title: "Send the invoice",
    dueDate: TODAY,
  });
  // Where the week starts is the platform's answer, not this parser's.
  expect(read("Ship the release next week")).toMatchObject({
    title: "Ship the release",
    dueDate: nextWeek(TODAY),
  });
  expect(read("Book the table friday")).toMatchObject({
    title: "Book the table",
    dueDate: "2026-09-25",
  });
  expect(read("Water the plants in 3 days")).toMatchObject({
    title: "Water the plants",
    dueDate: "2026-09-23",
  });
  expect(read("Renew the domain in 2 weeks")).toMatchObject({ dueDate: "2026-10-04" });
});

test("reads a clock time, in either order, and drops the connector with it", () => {
  expect(read("Call Anna tomorrow at 3pm")).toMatchObject({
    title: "Call Anna",
    dueDate: "2026-09-21",
    dueTime: "15:00",
  });
  expect(read("Call Anna at 3pm tomorrow")).toMatchObject({
    title: "Call Anna",
    dueDate: "2026-09-21",
    dueTime: "15:00",
  });
  expect(read("Standup at 09:30")).toMatchObject({ title: "Standup", dueTime: "09:30" });
  expect(read("Lunch at noon")).toMatchObject({ title: "Lunch", dueTime: "12:00" });
  expect(read("Deploy at midnight")).toMatchObject({ title: "Deploy", dueTime: "00:00" });
  expect(read("Call at 12am")).toMatchObject({ dueTime: "00:00" });
  expect(read("Call at 12pm")).toMatchObject({ dueTime: "12:00" });
  // A time needs a day to belong to, so a bare time means today.
  expect(read("Standup at 09:30").dueDate).toBe(TODAY);
});

test("an evening word carries its own hour, unless one was typed", () => {
  expect(read("Water the plants tonight")).toMatchObject({ dueDate: TODAY, dueTime: "20:00" });
  expect(read("Water the plants tonight at 9pm")).toMatchObject({ dueTime: "21:00" });
});

test("a bare calendar date means the next one, not one that has passed", () => {
  expect(read("Pay the rent 1 oct")).toMatchObject({ dueDate: "2026-10-01" });
  expect(read("Pay the rent oct 1")).toMatchObject({ dueDate: "2026-10-01" });
  expect(read("Renew on 3 march")).toMatchObject({ title: "Renew", dueDate: "2027-03-03" });
  expect(read("Archive 2026-12-24")).toMatchObject({ dueDate: "2026-12-24" });
});

test("a word in the middle of a line is never taken", () => {
  expect(read("Meet the Friday team")).toMatchObject({
    title: "Meet the Friday team",
    dueDate: null,
    matched: null,
  });
  expect(read("Plan tomorrow's standup")).toMatchObject({ dueDate: null });
  // The failure mode this parser is built to avoid.
  expect(read("Check market 5")).toMatchObject({ title: "Check market 5", dueDate: null });
  expect(read("Review PR 42")).toMatchObject({ dueDate: null, dueTime: null });
  expect(read("Call mom")).toMatchObject({ dueDate: null });
  expect(read("Fix the sat nav")).toMatchObject({ dueDate: null });
});

test("nonsense times are text, not times", () => {
  expect(read("Ship 25:00")).toMatchObject({ dueTime: null });
  expect(read("Ship 12:75")).toMatchObject({ dueTime: null });
  expect(read("Ship 13pm")).toMatchObject({ dueTime: null });
});

test("a line that is only a phrase keeps its words", () => {
  // Otherwise this is a task with no name at all.
  expect(read("tomorrow")).toMatchObject({ title: "tomorrow", dueDate: null, matched: null });
  expect(read("at 3pm")).toMatchObject({ title: "at 3pm", dueDate: null });
  expect(read("")).toMatchObject({ title: "", dueDate: null });
});

test("what it understood is reported back so the field can show it", () => {
  expect(read("Call Anna tomorrow at 3pm").matched).toBe("tomorrow at 3pm");
  expect(read("Standup at 09:30").matched).toBe("at 09:30");
  expect(read("Send the report   next   week")).toMatchObject({
    title: "Send the report",
    matched: "next week",
  });
});

test("sigil tokens read anywhere and leave the rest of the line alone", () => {
  const lists = [
    { id: "work", title: "Work" },
    { id: "review", title: "Tasks review" },
    { id: "reading", title: "Reading list" },
    { id: "research", title: "Research" },
  ];
  const read = parseTokens("Ship the draft !1 #tasks ~1h30m tomorrow", lists);
  expect(read).toEqual({
    rest: "Ship the draft tomorrow",
    priority: "high",
    list: "review",
    duration: 90,
    matched: { priority: "!1", list: "#tasks", duration: "~1h30m" },
  });
  // An ambiguous prefix, an unknown word and plain punctuation stay as text.
  expect(parseTokens("Wow ! #re ~soon", lists).rest).toBe("Wow ! #re ~soon");
  expect(parseTokens("Plan #work !low", lists, new Set(["list"]))).toMatchObject({
    rest: "Plan #work",
    priority: "low",
    list: null,
  });
});
