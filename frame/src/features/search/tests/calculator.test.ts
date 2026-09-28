import { describe, expect, it } from "vitest";
import { calculate } from "../lib/calculator";

const value = (query: string) => calculate(query, "en-US")?.value ?? null;

describe("launcher calculator", () => {
  it("follows precedence, grouping and powers", () => {
    expect(value("2+2*3")).toBe(8);
    expect(value("(2+2)*3")).toBe(12);
    expect(value("2^3^2")).toBe(512);
    expect(value("-2^2")).toBe(-4);
    expect(value("10 ÷ 4 × 2")).toBe(5);
    expect(value("3(4+5)")).toBe(27);
  });

  it("reads percentages, decimal commas, functions and constants", () => {
    expect(value("15% * 80")).toBe(12);
    expect(value("80 + 15%")).toBe(92);
    expect(value("200 - 10%")).toBe(180);
    expect(value("1,5 + 1")).toBe(2.5);
    expect(value("sqrt(16) + abs(-2)")).toBe(6);
    expect(value("2pi")).toBeCloseTo(6.283185307, 8);
    expect(value("round(2.5)")).toBe(3);
    expect(value("12*3=")).toBe(36);
  });

  it("hides binary noise and groups the answer for reading", () => {
    expect(value("0.1+0.2")).toBe(0.3);
    expect(calculate("1200*1000", "en-US")?.text).toBe("1,200,000");
  });

  it("leaves everything that is not arithmetic to search", () => {
    for (const query of [
      "",
      "2024",
      "-5",
      "e",
      "rust",
      "5 apples + 2",
      "notion.so",
      "localhost:3000",
      "1/0",
      "(2+3",
      "sqrt 4",
    ]) {
      expect(calculate(query), query).toBeNull();
    }
  });
});
