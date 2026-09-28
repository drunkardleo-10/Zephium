/** Arithmetic typed into the launcher, answered in place as the user types.
 *  A small recursive-descent parser: no `eval`, which the CSP forbids, and no
 *  dependency for what is a few dozen lines of grammar.
 *
 *  expression = term (("+" | "-") term)*
 *  term       = unary (("*" | "/" | "×" | "÷" | implicit) unary)*
 *  unary      = ("-" | "+") unary | power
 *  power      = postfix ("^" unary)?
 *  postfix    = primary "%"?
 *  primary    = number | constant | function "(" expression ")" | "(" expression ")"
 */

export type Calculation = { expression: string; value: number; text: string };

const FUNCTIONS: Record<string, (x: number) => number> = {
  sqrt: Math.sqrt,
  abs: Math.abs,
  round: Math.round,
  floor: Math.floor,
  ceil: Math.ceil,
  ln: Math.log,
  log: Math.log10,
  sin: Math.sin,
  cos: Math.cos,
  tan: Math.tan,
};
const CONSTANTS: Record<string, number> = { pi: Math.PI, π: Math.PI, e: Math.E };

type Token = { kind: "number"; value: number } | { kind: "word"; value: string } | { kind: string };

function tokenize(input: string): Token[] | null {
  const tokens: Token[] = [];
  let at = 0;
  while (at < input.length) {
    const char = input[at]!;
    if (/\s/u.test(char)) {
      at++;
      continue;
    }
    const number = /^(?:\d+(?:[.,]\d+)?|[.,]\d+)(?:e[+-]?\d+)?/iu.exec(input.slice(at));
    if (number) {
      // A comma between digits is a decimal separator, never a thousands
      // separator: "1,5" is one and a half, as most of the world writes it.
      tokens.push({ kind: "number", value: Number(number[0].replace(",", ".")) });
      at += number[0].length;
      continue;
    }
    const word = /^[a-zπ]+/iu.exec(input.slice(at));
    if (word) {
      tokens.push({ kind: "word", value: word[0].toLowerCase() });
      at += word[0].length;
      continue;
    }
    if ("+-*/×÷^%()".includes(char)) {
      tokens.push({ kind: char === "×" ? "*" : char === "÷" ? "/" : char });
      at++;
      continue;
    }
    return null;
  }
  return tokens;
}

function parse(tokens: Token[]): number | null {
  let at = 0;
  const peek = () => tokens[at];
  const take = (kind: string) => (peek()?.kind === kind ? tokens[at++] : undefined);

  // "80 + 15%" means fifteen percent more, as people and other calculators
  // read it, not eighty plus a fraction.
  function expression(): number {
    let value = term();
    for (;;) {
      const sign = take("+") ? 1 : take("-") ? -1 : 0;
      if (!sign) return value;
      const operand = term();
      value = tokens[at - 1]?.kind === "%" ? value * (1 + sign * operand) : value + sign * operand;
    }
  }
  function term(): number {
    let value = unary();
    for (;;) {
      if (take("*")) value *= unary();
      else if (take("/")) value /= unary();
      // "2pi" and "3(4+5)" multiply, as they are written on paper.
      else if (peek()?.kind === "(" || peek()?.kind === "word") value *= unary();
      else return value;
    }
  }
  // A power binds tighter than a leading sign: -2^2 is -4, as on paper.
  function unary(): number {
    if (take("-")) return -unary();
    if (take("+")) return unary();
    return power();
  }
  function power(): number {
    const base = postfix();
    return take("^") ? base ** unary() : base;
  }
  function postfix(): number {
    const value = primary();
    return take("%") ? value / 100 : value;
  }
  function primary(): number {
    const token = tokens[at++];
    if (!token) throw new Error("end");
    if (token.kind === "number") return (token as { value: number }).value;
    if (token.kind === "(") {
      const value = expression();
      if (!take(")")) throw new Error("unclosed");
      return value;
    }
    if (token.kind === "word") {
      const name = (token as { value: string }).value;
      const fn = FUNCTIONS[name];
      if (fn) {
        if (!take("(")) throw new Error("call");
        const value = expression();
        if (!take(")")) throw new Error("unclosed");
        return fn(value);
      }
      if (name in CONSTANTS) return CONSTANTS[name]!;
    }
    throw new Error("unexpected");
  }

  try {
    const value = expression();
    return at === tokens.length ? value : null;
  } catch {
    return null;
  }
}

/** The answer to `query` when it is arithmetic, or null. A bare number, a
 *  word, or anything that does not compute is left to search: "2024" is a
 *  year and "e" is a letter, not questions. */
export function calculate(query: string, locale?: string): Calculation | null {
  const expression = query.trim().replace(/=$/u, "").trim();
  if (!expression || expression.length > 256) return null;
  const tokens = tokenize(expression);
  if (!tokens?.length) return null;
  const computes = tokens.some(
    (token) =>
      "+-*/^%".includes(token.kind) ||
      (token.kind === "word" &&
        ((token as { value: string }).value in FUNCTIONS ||
          (token as { value: string }).value in CONSTANTS)),
  );
  if (!computes || !tokens.some((token) => token.kind === "number")) return null;
  // "-5" is a number with a sign, not a sum to work out.
  if (tokens.length === 2 && "+-".includes(tokens[0]!.kind)) return null;
  const raw = parse(tokens);
  if (raw === null || !Number.isFinite(raw)) return null;
  // Twelve significant digits hide binary noise: 0.1 + 0.2 is 0.3.
  const value = Number(raw.toPrecision(12));
  return {
    expression,
    value,
    text: new Intl.NumberFormat(locale, { maximumFractionDigits: 10 }).format(value),
  };
}
