import { expect, test } from "vitest";
import { modelName } from "../lib/model-name";

test("a model id reads as its name", () => {
  expect(modelName("gpt-5.6-luna")).toBe("GPT-5.6 Luna");
  expect(modelName("gpt-5.6-terra")).toBe("GPT-5.6 Terra");
  expect(modelName("sonnet")).toBe("Sonnet");
});
