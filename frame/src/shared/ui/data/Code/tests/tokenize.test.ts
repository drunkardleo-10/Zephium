import { expect, test } from "vitest";
import { tokenize, type Token } from "../tokenize";

const kinds = (row: Token[] | undefined, kind: Token["kind"]) =>
  (row ?? []).filter((token) => token.kind === kind).map((token) => token.text.trim());

test("rust: keywords, strings, numbers and line comments", () => {
  const [row] = tokenize(
    "rust",
    'pub fn size(x: u32) -> u32 { let s = "a\\"b"; x * 0x1F } // done',
  );
  expect(kinds(row, "keyword")).toEqual(["pub", "fn", "let"]);
  expect(kinds(row, "string")).toEqual(['"a\\"b"']);
  expect(kinds(row, "number")).toEqual(["0x1F"]);
  expect(kinds(row, "comment")).toEqual(["// done"]);
  expect(row?.map((token) => token.text).join("")).toBe(
    'pub fn size(x: u32) -> u32 { let s = "a\\"b"; x * 0x1F } // done',
  );
});

test("script family shares one table and template strings", () => {
  for (const language of ["typescript", "javascript", "svelte"]) {
    const [row] = tokenize(language, "const n = `t${1}`; return await f(2.5e3);");
    expect(kinds(row, "keyword")).toEqual(["const", "return", "await"]);
    expect(kinds(row, "string")).toEqual(["`t${1}`"]);
    expect(kinds(row, "number")).toEqual(["2.5e3"]);
  }
});

test("hash comment families", () => {
  expect(kinds(tokenize("python", "def f(): return None  # why")[0], "comment")).toEqual(["# why"]);
  expect(kinds(tokenize("python", "def f(): return None")[0], "keyword")).toEqual([
    "def",
    "return",
    "None",
  ]);
  expect(kinds(tokenize("ruby", "def x; end # c")[0], "keyword")).toEqual(["def", "end"]);
  expect(kinds(tokenize("bash", 'if [ -f "$f" ]; then echo ok; fi')[0], "keyword")).toEqual([
    "if",
    "then",
    "echo",
    "fi",
  ]);
  expect(kinds(tokenize("yaml", "on: true # x")[0], "keyword")).toEqual(["on", "true"]);
  expect(kinds(tokenize("toml", 'name = "z" # x')[0], "string")).toEqual(['"z"']);
});

test("c, jvm, go and php families", () => {
  expect(kinds(tokenize("cpp", "static int n = 42; // x")[0], "keyword")).toEqual([
    "static",
    "int",
  ]);
  expect(kinds(tokenize("csharp", "public class A {}")[0], "keyword")).toEqual(["public", "class"]);
  expect(kinds(tokenize("kotlin", "val x = when (y) {}")[0], "keyword")).toEqual(["val", "when"]);
  expect(kinds(tokenize("swift", "guard let x else { return }")[0], "keyword")).toEqual([
    "guard",
    "let",
    "else",
    "return",
  ]);
  expect(kinds(tokenize("go", "func main() { defer f() }")[0], "keyword")).toEqual([
    "func",
    "defer",
  ]);
  expect(kinds(tokenize("php", "echo $x; # note")[0], "comment")).toEqual(["# note"]);
});

test("sql and dockerfile keywords ignore case", () => {
  expect(kinds(tokenize("sql", "SELECT id FROM t WHERE n > 1 -- rows")[0], "keyword")).toEqual([
    "SELECT",
    "FROM",
    "WHERE",
  ]);
  expect(kinds(tokenize("dockerfile", "FROM node:24 AS build")[0], "keyword")).toEqual([
    "FROM",
    "AS",
  ]);
});

test("minimal and unknown languages", () => {
  expect(kinds(tokenize("json", '{"a": [1, true, null]}')[0], "keyword")).toEqual(["true", "null"]);
  expect(kinds(tokenize("css", "a { width: 12px; } /* c */")[0], "comment")).toEqual(["/* c */"]);
  expect(kinds(tokenize("html", '<p class="x"><!-- c --></p>')[0], "comment")).toEqual([
    "<!-- c -->",
  ]);
  expect(tokenize("markdown", "# Title")[0]).toEqual([
    { kind: "punctuation", text: "#" },
    { kind: "plain", text: " Title" },
  ]);
  expect(tokenize("text", "fn if 1")).toEqual([[{ kind: "plain", text: "fn if 1" }]]);
  expect(tokenize("klingon", "a\n\nb")).toEqual([
    [{ kind: "plain", text: "a" }],
    [],
    [{ kind: "plain", text: "b" }],
  ]);
});

test("unbalanced strings end at the line and block comments carry until closed", () => {
  const rows = tokenize("typescript", 'const s = "open\nlet x = 1; /* a\nstill */ let y\nlet z');
  expect(kinds(rows[0], "string")).toEqual(['"open']);
  expect(kinds(rows[1], "keyword")).toEqual(["let"]);
  expect(kinds(rows[1], "comment")).toEqual(["/* a"]);
  expect(kinds(rows[2], "comment")).toEqual(["still */"]);
  expect(kinds(rows[2], "keyword")).toEqual(["let"]);
  expect(kinds(rows[3], "keyword")).toEqual(["let"]);
  expect(kinds(tokenize("python", "x = 'a\\")[0], "string")).toEqual(["'a\\"]);
});

test("a limit tokenizes only the leading lines and odd input never throws", () => {
  expect(tokenize("rust", "a\nb\nc", 2)).toHaveLength(2);
  expect(tokenize("rust", "a\r\nb\n")).toHaveLength(2);
  expect(() => tokenize("rust", "\u0000é😀\\\"'`/*")).not.toThrow();
  expect(
    tokenize("rust", "😀 é")[0]
      ?.map((token) => token.text)
      .join(""),
  ).toBe("😀 é");
});
