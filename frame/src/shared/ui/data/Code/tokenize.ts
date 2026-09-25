import { codeLines } from "./code";

type TokenKind = "keyword" | "string" | "comment" | "number" | "punctuation" | "plain";
export type Token = { kind: TokenKind; text: string };

type Spec = {
  words: Set<string>;
  line: string[];
  block?: [string, string];
  quotes: string;
  fold?: boolean;
};

const C_LIKE = "if else for while do switch case default break continue return goto";
const TYPES_C =
  "int long short char float double void bool unsigned signed const static struct enum union";
const SHARED = {
  rust: `as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while yield`,
  script: `${C_LIKE} as async await catch class const debugger delete export extends false finally from function get import in instanceof interface keyof let new null of readonly satisfies set static super this throw true try type typeof undefined var void with yield enum implements declare abstract private protected public each snippet`,
  python: `False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda match case nonlocal not or pass raise return self try while with yield`,
  go: `break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var true false nil`,
  jvm: `${C_LIKE} abstract class extends final finally implements import instanceof interface new null package private protected public static super this throw throws try true false var val fun object when is in let guard func struct enum protocol extension init self Self nil`,
  c: `${C_LIKE} ${TYPES_C} auto extern register sizeof typedef volatile inline true false NULL nullptr class namespace template typename public private protected virtual override new delete this using include define ifdef ifndef endif pragma try catch throw base var null`,
  ruby: `BEGIN END alias and begin break case class def defined do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield require attr_reader attr_accessor`,
  php: `${C_LIKE} abstract array as catch class clone const echo extends final finally fn foreach function global implements include instanceof interface match namespace new null private protected public require static throw trait true false try use var yield`,
  sql: `select from where and or not insert into values update set delete create table alter drop index join inner left right outer full on group by order having limit offset as distinct union all null is in like between case when then else end primary key foreign references default exists with returning asc desc count sum avg min max`,
  bash: `if then else elif fi for while until do done case esac in function return local export readonly declare unset shift exit echo source true false`,
  docker: `from run cmd label expose env add copy entrypoint volume user workdir arg onbuild stopsignal healthcheck shell as`,
  data: `true false null yes no on off`,
};

const lang = (
  words: string,
  line: string[],
  block: [string, string] | undefined,
  quotes: string,
  fold?: boolean,
): Spec => ({
  words: new Set(words.split(" ")),
  line,
  block,
  quotes,
  fold,
});

const SLASH = ["//"];
const STAR: [string, string] = ["/*", "*/"];
const script = lang(SHARED.script, SLASH, STAR, "\"'`");
const jvm = lang(SHARED.jvm, SLASH, STAR, "\"'");
const c = lang(SHARED.c, SLASH, STAR, "\"'");
const data = lang(SHARED.data, ["#"], undefined, "\"'");

const SPECS: Record<string, Spec> = {
  rust: lang(SHARED.rust, SLASH, STAR, '"'),
  typescript: script,
  javascript: script,
  svelte: script,
  python: lang(SHARED.python, ["#"], undefined, "\"'"),
  go: lang(SHARED.go, SLASH, STAR, "\"'`"),
  java: jvm,
  kotlin: jvm,
  swift: jvm,
  c,
  cpp: c,
  csharp: c,
  ruby: lang(SHARED.ruby, ["#"], undefined, "\"'"),
  php: lang(SHARED.php, ["//", "#"], STAR, "\"'"),
  sql: lang(SHARED.sql, ["--"], STAR, "'\"", true),
  bash: lang(SHARED.bash, ["#"], undefined, "\"'"),
  css: lang("", [], STAR, "\"'"),
  html: lang("", [], ["<!--", "-->"], "\"'"),
  json: lang(SHARED.data, [], undefined, '"'),
  yaml: data,
  toml: data,
  markdown: lang("", [], ["<!--", "-->"], ""),
  dockerfile: lang(SHARED.docker, ["#"], undefined, "\"'", true),
};

const WORD = /[A-Za-z_$][\w$]*/y;
const NUMBER = /(?:0[xob][\da-f_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:e[+-]?\d+)?)[a-z\d_]*/iy;
const SPACE = /\s+/y;
const MAX_TEXT = 1 << 17;

const PATTERNS = [
  [SPACE, "plain"],
  [WORD, "word"],
  [NUMBER, "number"],
] as const;

function scan(line: string, at: number, spec: Spec): Token {
  for (const [pattern, kind] of PATTERNS) {
    pattern.lastIndex = at;
    const text = pattern.exec(line)?.[0];
    if (!text) continue;
    if (kind !== "word") return { kind, text };
    return {
      kind: spec.words.has(spec.fold ? text.toLowerCase() : text) ? "keyword" : "plain",
      text,
    };
  }
  return { kind: "punctuation", text: line[at]! };
}

export function tokenize(language: string, text: string, limit = Infinity): Token[][] {
  const rows = codeLines(text.slice(0, MAX_TEXT)).slice(0, Math.max(0, limit));
  const spec = SPECS[language];
  if (!spec) return rows.map((line) => (line ? [{ kind: "plain", text: line }] : []));
  let open = false;
  return rows.map((line) => {
    const row: Token[] = [];
    const push = (kind: TokenKind, value: string) => {
      const last = row.at(-1);
      if (last?.kind === kind) last.text += value;
      else if (value) row.push({ kind, text: value });
    };
    let i = 0;
    while (i < line.length) {
      if (open && spec.block) {
        const end = line.indexOf(spec.block[1], i);
        const stop = end < 0 ? line.length : end + spec.block[1].length;
        open = end < 0;
        push("comment", line.slice(i, stop));
        i = stop;
        continue;
      }
      if (spec.line.some((mark) => line.startsWith(mark, i))) {
        push("comment", line.slice(i));
        break;
      }
      if (spec.block && line.startsWith(spec.block[0], i)) {
        open = true;
        push("comment", spec.block[0]);
        i += spec.block[0].length;
        continue;
      }
      const ch = line[i]!;
      if (spec.quotes.includes(ch)) {
        let j = i + 1;
        while (j < line.length && line[j] !== ch) j += line[j] === "\\" ? 2 : 1;
        const stop = Math.min(j + 1, line.length);
        push("string", line.slice(i, stop));
        i = stop;
        continue;
      }
      const next = scan(line, i, spec);
      push(next.kind, next.text);
      i += next.text.length;
    }
    return row;
  });
}
