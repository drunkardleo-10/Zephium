import { codeLines } from "./code";

/** What a stretch of code is painted as; everything else is plain text. */
export const CODE_KINDS = [
  "comment",
  "string",
  "number",
  "keyword",
  "function",
  "type",
  "property",
  "tag",
  "attribute",
] as const;
type CodeKind = (typeof CODE_KINDS)[number];
export type Token = { kind: CodeKind | "plain"; text: string };

const [COMMENT, STRING, NUMBER, KEYWORD, FUNCTION, TYPE, PROPERTY, TAG, ATTRIBUTE] = [
  0, 1, 2, 3, 4, 5, 6, 7, 8,
] as const;

/**
 * How a family of languages is read: its comments, which quotes open a
 * string on one line and which may run on (`long`), its keywords and
 * built-in types, and the few rules a family adds: case-insensitive
 * keywords, `'x'` as a character (a lifetime otherwise), PascalCase names as
 * types, keys before `:` or `=` as properties, tags (HTML, Svelte, JSX) and
 * CSS's own shape.
 */
type Family = {
  line: readonly string[];
  block: readonly [string, string] | null;
  quotes: string;
  long: string;
  words: ReadonlySet<string>;
  types: ReadonlySet<string>;
  fold?: boolean;
  char?: boolean;
  pascal?: boolean;
  keys?: ":" | "=";
  markup?: "html" | "jsx";
  css?: boolean;
};

const set = (words: string) => new Set(words.split(" ").filter(Boolean));
const LOOP = "if else for while do switch case default break continue return";
const SCRIPT = set(
  `${LOOP} as async await catch class const debugger delete export extends false finally from function get import in instanceof interface keyof let new null of readonly satisfies set static super this throw true try type typeof undefined var void with yield enum implements declare abstract private protected public`,
);
const GENERIC = set(
  `${LOOP} and as assert async await catch class const crate def defer del delete do dyn elif end enum except export extends extern false final finally fn from func function go guard impl implements import in instanceof interface is lambda let local loop match mod module mut namespace new nil none not null of or override package pass private protected pub public raise readonly select self static struct super then this throw throws trait true try type typeof undefined unless unsafe use using val var void when where with yield`,
);
const SLASH = ["//"];
const HASH = ["#"];
const STAR = ["/*", "*/"] as const;
const NONE = new Set<string>();

const family = (spec: Partial<Family> & Pick<Family, "words">): Family => ({
  line: [],
  block: null,
  quotes: "\"'",
  long: "",
  types: NONE,
  ...spec,
});
const script = family({
  line: SLASH,
  block: STAR,
  long: "`",
  words: SCRIPT,
  types: set("string number boolean any unknown never object bigint symbol"),
  pascal: true,
});
const jsx = { ...script, markup: "jsx" as const };
const clike = family({
  line: SLASH,
  block: STAR,
  words: set(
    `${LOOP} auto extern register sizeof typedef volatile inline true false NULL nullptr class namespace template typename public private protected virtual override new delete this using include define ifdef ifndef endif pragma try catch throw base var null static const struct enum union goto`,
  ),
  types: set("int long short char float double void bool unsigned signed size_t string byte"),
  pascal: true,
});
const jvm = family({
  line: SLASH,
  block: STAR,
  long: '"',
  words: set(
    `${LOOP} abstract class extends final finally implements import instanceof interface new null package private protected public static super this throw throws try true false var val fun object when is in let guard func struct enum protocol extension init self Self nil override open data sealed companion async await`,
  ),
  types: set("int long short char float double void boolean byte"),
  pascal: true,
});
const hash = family({ line: HASH, words: GENERIC, pascal: true });
const dash = family({
  line: ["--"],
  block: ["{-", "-}"],
  words: set(
    `and break do else elseif end false for function goto if in local nil not or repeat return then true until while case class data deriving import infix instance let module newtype of type where`,
  ),
  pascal: true,
});
const data = set("true false null yes no on off");
const html = family({ block: ["<!--", "-->"], words: NONE, markup: "html" });
const css = family({ block: STAR, words: NONE, css: true });

const FAMILIES: Record<string, Family> = {
  script,
  jsx,
  rust: family({
    line: SLASH,
    block: STAR,
    quotes: "\"'",
    long: '"',
    words: set(
      "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while yield",
    ),
    types: set("u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64 bool char str"),
    char: true,
    pascal: true,
  }),
  python: family({
    line: HASH,
    words: set(
      "False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda match case nonlocal not or pass raise return self try while with yield",
    ),
    types: set("int str float bool list dict set tuple bytes object"),
    pascal: true,
  }),
  go: family({
    line: SLASH,
    block: STAR,
    long: "`",
    words: set(
      "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var true false nil",
    ),
    types: set(
      "int int8 int16 int32 int64 uint uint8 uint16 uint32 uint64 uintptr float32 float64 string bool byte rune error any",
    ),
    pascal: true,
  }),
  jvm,
  c: clike,
  ruby: family({
    line: HASH,
    words: set(
      "BEGIN END alias and begin break case class def defined do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield require attr_reader attr_accessor",
    ),
    pascal: true,
  }),
  php: family({
    line: ["//", "#"],
    block: STAR,
    words: set(
      `${LOOP} abstract array as catch class clone const echo extends final finally fn foreach function global implements include instanceof interface match namespace new null private protected public require static throw trait true false try use var yield`,
    ),
    pascal: true,
  }),
  sql: family({
    line: ["--"],
    block: STAR,
    words: set(
      "select from where and or not insert into values update set delete create table alter drop index join inner left right outer full on group by order having limit offset as distinct union all null is in like between case when then else end primary key foreign references default exists with returning asc desc",
    ),
    types: set(
      "int integer bigint text varchar char boolean timestamp timestamptz date uuid jsonb json serial numeric real",
    ),
    fold: true,
  }),
  shell: family({
    line: HASH,
    words: set(
      "if then else elif fi for while until do done case esac in function return local export readonly declare unset shift exit echo source true false",
    ),
  }),
  docker: family({
    line: HASH,
    words: set(
      "from run cmd label expose env add copy entrypoint volume user workdir arg onbuild stopsignal healthcheck shell as",
    ),
    fold: true,
  }),
  hash,
  dash,
  css,
  scss: { ...css, line: SLASH },
  html,
  json: family({ quotes: '"', words: data, keys: ":" }),
  jsonc: family({ line: SLASH, block: STAR, quotes: '"', words: data, keys: ":" }),
  yaml: family({ line: HASH, words: data, keys: ":" }),
  toml: family({ line: HASH, words: data, keys: "=" }),
  markdown: family({ block: ["<!--", "-->"], quotes: "", words: NONE }),
};

/** The names a fence or a file calls each family by. */
const NAMES: Record<string, string> = {
  script: "ts typescript mts cts javascript js mjs cjs",
  jsx: "jsx tsx",
  rust: "rs rust",
  python: "py python",
  go: "go golang",
  jvm: "java kotlin kt kts swift scala dart groovy",
  c: "c h cpp c++ cc hpp cs csharp objc zig",
  ruby: "rb ruby",
  php: "php",
  sql: "sql psql mysql sqlite postgres postgresql",
  shell: "sh bash zsh shell console fish",
  docker: "dockerfile docker",
  hash: "r perl pl make makefile ini conf elixir ex exs nix powershell ps1",
  dash: "lua hs haskell elm",
  css: "css",
  scss: "scss less",
  html: "html htm xml svg svelte vue",
  json: "json json5",
  jsonc: "jsonc",
  yaml: "yaml yml",
  toml: "toml",
  markdown: "md markdown",
};
const BY_NAME = new Map<string, Family>();
for (const [name, names] of Object.entries(NAMES))
  for (const alias of names.split(" ")) BY_NAME.set(alias, FAMILIES[name]!);

/** The family a language (or a fence's info string) names, if this reads it. */
export function languageOf(language: unknown): Family | null {
  if (typeof language !== "string") return null;
  return BY_NAME.get(language.trim().split(/\s/u, 1)[0]!.toLowerCase()) ?? null;
}

const digit = (c: number) => c >= 48 && c <= 57;
const alpha = (c: number) => (c >= 65 && c <= 90) || (c >= 97 && c <= 122);
const start = (c: number) => alpha(c) || c === 95 || c === 36 || c >= 0xc0;
const word = (c: number) => start(c) || digit(c);
const space = (c: number) => c === 32 || c === 9 || c === 13;
/** A tag's or an attribute's name runs until a space or its punctuation. */
const named = (c: number) =>
  c > 32 && c !== 61 && c !== 62 && c !== 47 && c !== 34 && c !== 39 && c !== 123 && c !== 125;
/** Before these, `<` opens a JSX element rather than comparing. */
const OPENS = new Set([0, 40, 61, 44, 123, 125, 58, 63, 38, 124, 62, 59, 91]);

/**
 * Comments, strings, numbers, keywords, function calls, types, properties,
 * tags and attributes in `text`, as flat `[start, end, kind]` triples in
 * order (kind indexes `CODE_KINDS`); what they leave is plain text. One pass,
 * no backtracking; a language this does not read yields none.
 */
export function scanCode(text: string, language: string | Family): number[] {
  const spec = typeof language === "string" ? languageOf(language) : language;
  const out: number[] = [];
  if (!spec) return out;
  const end = text.length;
  const at = (i: number) => text.charCodeAt(i);
  const emit = (from: number, to: number, kind: number) => {
    if (to > from && kind >= 0) out.push(from, to, kind);
  };
  const eol = (i: number) => {
    const next = text.indexOf("\n", i);
    return next < 0 ? end : next;
  };
  const after = (i: number) => {
    while (i < end && space(at(i))) i += 1;
    return i;
  };

  /**
   * Where a string from its quote ends, past the closing one; -1 while it is
   * still open (on its line, unless it may run on), so a quote being typed
   * does not paint the rest of the block.
   */
  function quoted(i: number, long: boolean): number {
    const quote = at(i);
    let j = i + 1;
    while (j < end) {
      const c = at(j);
      if (c === 92) j += 2;
      else if (c === quote) return j + 1;
      else if (c === 10 && !long) return -1;
      else j += 1;
    }
    return -1;
  }

  /** Code until the end, a `}` that closes nothing opened here, or `close` (`</script`). */
  function code(f: Family, i: number, close: string): number {
    const opens = new Set(
      [...f.line, ...(f.block ? [f.block[0]] : [])].map((m) => m.charCodeAt(0)),
    );
    let depth = 0;
    let nest = 0;
    let prev = 0;
    let last = "";
    let fresh = true;
    while (i < end) {
      const c = at(i);
      if (c === 10) {
        fresh = true;
        i += 1;
        continue;
      }
      if (space(c)) {
        i += 1;
        continue;
      }
      if (close === "}" && c === 123) depth += 1;
      else if (close === "}" && c === 125 && !depth--) return i;
      else if (close && c === 60 && text.slice(i, i + close.length).toLowerCase() === close)
        return i;
      if (opens.has(c)) {
        const mark = f.line.find((m) => text.startsWith(m, i));
        if (mark) {
          const stop = eol(i);
          emit(i, stop, COMMENT);
          i = stop;
          continue;
        }
        if (f.block && text.startsWith(f.block[0], i)) {
          const found = text.indexOf(f.block[1], i + f.block[0].length);
          const stop = found < 0 ? end : found + f.block[1].length;
          emit(i, stop, COMMENT);
          i = stop;
          continue;
        }
      }
      const ch = text[i]!;
      if (f.quotes.includes(ch) || f.long.includes(ch)) {
        // Rust: 'x' is a character, 'a alone a lifetime.
        if (f.char && c === 39 && at(i + 2) !== 39 && at(i + 1) !== 92) {
          prev = c;
          i += 1;
          continue;
        }
        const stop = quoted(i, f.long.includes(ch));
        if (stop > 0) {
          emit(i, stop, f.keys === ":" && at(after(stop)) === 58 ? PROPERTY : STRING);
          prev = 34;
          fresh = false;
          i = stop;
          continue;
        }
      }
      if (digit(c) || (c === 46 && digit(at(i + 1)) && !word(prev))) {
        let j = i + 1;
        while (
          j < end &&
          (word(at(j)) ||
            (at(j) === 46 && digit(at(j + 1))) ||
            ((at(j) === 43 || at(j) === 45) && (at(j - 1) | 32) === 101 && at(i + 1) !== 120))
        )
          j += 1;
        emit(i, j, NUMBER);
        prev = 48;
        fresh = false;
        i = j;
        continue;
      }
      if (
        f.css &&
        (c === 64 || c === 35 || c === 46 || c === 58) &&
        (start(at(i + 1)) || digit(at(i + 1)) || at(i + 1) === 45)
      ) {
        // `@media`, `#fff` or `#id`, `.class`, `:hover`.
        let j = i + 1;
        while (j < end && (word(at(j)) || at(j) === 45)) j += 1;
        const kind =
          c === 64 ? KEYWORD : c === 35 && nest ? NUMBER : c === 58 && nest ? -1 : ATTRIBUTE;
        if (kind >= 0) {
          emit(i, j, kind);
          i = j;
          prev = 97;
          continue;
        }
      }
      if (start(c) || (f.css && c === 45 && start(at(i + 1)))) {
        let j = i + 1;
        while (j < end && (word(at(j)) || (f.css && at(j) === 45))) j += 1;
        const name = text.slice(i, j);
        const next = after(j);
        let kind = -1;
        if (f.css) {
          if (nest && at(next) === 58) kind = PROPERTY;
          else if (at(next) === 40) kind = FUNCTION;
          else if (!nest) kind = TAG;
          else if (prev === 33) kind = KEYWORD;
        } else if (prev !== 46 && (f.fold ? f.words.has(name.toLowerCase()) : f.words.has(name)))
          kind = KEYWORD;
        else if (f.types.has(name)) kind = TYPE;
        else if (
          at(next) === 40 ||
          (f.char && at(j) === 33 && [40, 91, 123].includes(at(after(j + 1))))
        )
          kind = FUNCTION;
        else if (prev === 46 && at(i - 2) !== 46) kind = PROPERTY;
        else if (f.keys && fresh && at(next) === f.keys.charCodeAt(0)) kind = PROPERTY;
        else if (f.pascal && c >= 65 && c <= 90 && /[a-z]/u.test(name)) kind = TYPE;
        emit(i, j, kind);
        last = name;
        prev = 97;
        fresh = false;
        i = j;
        continue;
      }
      if (
        f.markup === "jsx" &&
        c === 60 &&
        (alpha(at(i + 1)) || at(i + 1) === 62) &&
        (OPENS.has(prev) || last === "return")
      ) {
        i = element(f, i);
        prev = 62;
        last = "";
        continue;
      }
      if (f.css && c === 123) nest += 1;
      if (f.css && c === 125) nest = Math.max(0, nest - 1);
      prev = c;
      last = "";
      fresh = false;
      i += 1;
    }
    return i;
  }

  /** One tag from its `<`: its name, its attributes and their values, `{…}` read as script. */
  function tag(host: Family, i: number) {
    let j = i + 1;
    const closing = at(j) === 47;
    if (closing) j += 1;
    const from = j;
    while (j < end && named(at(j))) j += 1;
    const name = text.slice(from, j).toLowerCase();
    emit(from, j, TAG);
    let self = false;
    while (j < end) {
      const c = at(j);
      if (c === 62) {
        j += 1;
        break;
      }
      if (c === 47 && at(j + 1) === 62) {
        self = true;
        j += 2;
        break;
      }
      if (c === 123) {
        j = Math.min(end, code(host.markup === "jsx" ? host : script, j + 1, "}") + 1);
      } else if (c === 34 || c === 39) {
        const stop = quoted(j, true);
        emit(j, stop, STRING);
        j = stop > 0 ? stop : j + 1;
      } else if (named(c)) {
        const stop = j;
        while (j < end && named(at(j))) j += 1;
        emit(stop, j, ATTRIBUTE);
      } else j += 1;
    }
    return { end: j, closing, self, name };
  }

  /** A JSX element and its children, back to where its matching close ends. */
  function element(host: Family, i: number): number {
    let depth = 0;
    while (i < end) {
      const opened = tag(host, i);
      i = opened.end;
      if (opened.closing) depth -= 1;
      else if (!opened.self) depth += 1;
      if (depth <= 0) return i;
      while (i < end) {
        const c = at(i);
        if (c === 123) i = Math.min(end, code(host, i + 1, "}") + 1);
        else if (c === 60 && (alpha(at(i + 1)) || at(i + 1) === 47 || at(i + 1) === 62)) break;
        else i += 1;
      }
    }
    return i;
  }

  /** A page of markup: tags, comments, `{…}` blocks (Svelte, Vue), a script's or a style's body. */
  function markup(f: Family, i: number) {
    while (i < end) {
      const c = at(i);
      if (c === 60 && text.startsWith("<!--", i)) {
        const found = text.indexOf("-->", i + 4);
        const stop = found < 0 ? end : found + 3;
        emit(i, stop, COMMENT);
        i = stop;
      } else if (c === 60 && at(i + 1) === 33) {
        const stop = text.indexOf(">", i);
        emit(i + 1, stop < 0 ? end : stop, KEYWORD);
        i = stop < 0 ? end : stop + 1;
      } else if (c === 60 && (alpha(at(i + 1)) || at(i + 1) === 47)) {
        const opened = tag(f, i);
        i = opened.end;
        if (!opened.closing && !opened.self && opened.name === "script")
          i = code(script, i, "</script");
        else if (!opened.closing && !opened.self && opened.name === "style")
          i = code(css, i, "</style");
      } else if (c === 123) {
        // Svelte's `{#if …}`, `{:else}`, `{/each}`, `{@render …}`: the block's word is a keyword.
        let j = at(i + 1) === 123 ? i + 2 : i + 1;
        if ("#:/@".includes(text[j] ?? "")) {
          const from = j;
          j += 1;
          while (j < end && word(at(j))) j += 1;
          emit(from, j, KEYWORD);
        }
        i = Math.min(end, code(script, j, "}") + 1);
      } else i += 1;
    }
  }

  if (spec.markup === "html") markup(spec, 0);
  else code(spec, 0, "");
  return out;
}

/** What a highlighter reads at most; past it, code stays plain. */
const MAX_TEXT = 1 << 17;

/**
 * Code as rows of tokens, one row per line, for a static block: the scan's
 * stretches cut at each line's end (a comment or a string may run across
 * lines), plain text between. `limit` reads only the leading lines.
 */
export function tokenize(language: string, text: string, limit = Infinity): Token[][] {
  const source = text.slice(0, MAX_TEXT).replace(/\r\n/gu, "\n");
  const rows = codeLines(source).slice(0, Math.max(0, limit));
  const length = rows.reduce((sum, row) => sum + row.length + 1, 0);
  const marks = scanCode(source.slice(0, length), language);
  let mark = 0;
  let offset = 0;
  return rows.map((line) => {
    const row: Token[] = [];
    const from = offset;
    const to = from + line.length;
    let at = from;
    const plain = (upto: number) => {
      if (upto > at) row.push({ kind: "plain", text: source.slice(at, upto) });
    };
    while (mark < marks.length && marks[mark]! < to) {
      const start = Math.max(marks[mark]!, from);
      const stop = Math.min(marks[mark + 1]!, to);
      plain(start);
      if (stop > start)
        row.push({ kind: CODE_KINDS[marks[mark + 2]!]!, text: source.slice(start, stop) });
      at = Math.max(at, stop);
      if (marks[mark + 1]! > to) break;
      mark += 3;
    }
    plain(to);
    offset = to + 1;
    return row;
  });
}
