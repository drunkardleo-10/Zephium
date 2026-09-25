import { describe, expect, test } from "vitest";
import { familyOf, KINDS, scan } from "../lib/editor/highlight";

function tokens(code: string, language: string) {
  const family = familyOf(language)!;
  const flat = scan(code, family);
  const out: [string, string][] = [];
  for (let at = 0; at < flat.length; at += 3)
    out.push([KINDS[flat[at + 2]!]!, code.slice(flat[at], flat[at + 1])]);
  return out;
}

describe("code scanning", () => {
  test("names a family only for a language it knows", () => {
    expect(familyOf("TS")).not.toBeNull();
    expect(familyOf("python extra words")).not.toBeNull();
    expect(familyOf("klingon")).toBeNull();
    expect(familyOf(null)).toBeNull();
  });

  test("finds comments, strings, numbers and keywords, and nothing else", () => {
    expect(tokens('const url = "/a"; // why\nreturn 0x1F + 2.5e3;', "ts")).toEqual([
      ["keyword", "const"],
      ["string", '"/a"'],
      ["comment", "// why"],
      ["keyword", "return"],
      ["number", "0x1F"],
      ["number", "2.5e3"],
    ]);
  });

  test("reads comments the way each language writes them", () => {
    expect(tokens("x = 1  # note", "py")).toEqual([
      ["number", "1"],
      ["comment", "# note"],
    ]);
    expect(tokens("SELECT 1 -- note", "sql")).toEqual([
      ["keyword", "SELECT"],
      ["number", "1"],
      ["comment", "-- note"],
    ]);
    // `#` is a colour in CSS, not a comment.
    expect(tokens("a { color: #fff } /* c */", "css")).toEqual([["comment", "/* c */"]]);
  });

  test("leaves a quote open until it closes on its line", () => {
    expect(tokens('let s = "unfinished\nlet t = 1', "js")).toEqual([
      ["keyword", "let"],
      ["keyword", "let"],
      ["number", "1"],
    ]);
  });

  test("reads a Rust lifetime as a name, not a string", () => {
    expect(tokens("fn f<'a>(x: &'a str) -> char { 'x' }", "rust")).toEqual([
      ["keyword", "fn"],
      ["string", "'x'"],
    ]);
  });

  test("stays linear on a long block", () => {
    const code = 'const value = "text"; // comment 42\n'.repeat(5000);
    const started = performance.now();
    expect(scan(code, familyOf("ts")!).length).toBe(5000 * 3 * 3);
    expect(performance.now() - started).toBeLessThan(250);
  });
});
