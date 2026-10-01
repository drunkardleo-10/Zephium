import { expect, test } from "vitest";
import type { WorkPageV1 } from "$shared/ipc/bindings";
import { subjectDetail, subjectPage } from "../lib/subject-detail";
import { projection } from "./environment-fixtures";

const read = (url: string, framed = true): WorkPageV1 => ({
  execution: "execution",
  attempt: "attempt",
  step: url.length.toString(),
  url,
  live: false,
  frame: framed ? { generation: 1, width: 1280, height: 800 } : null,
});

test("a subject stands on its homepage's frame, else a cited page's, else its site's", () => {
  const home = read("https://www.stripe.com/");
  const cited = read("https://news.example/stripe");
  const site = read("https://stripe.com/pricing");
  const source = { key: "a", label: "News", url: "https://news.example/stripe#top" };
  expect(subjectPage([cited, home], "https://stripe.com", [source])).toBe(home);
  expect(subjectPage([site, cited], "https://stripe.com", [source])).toBe(cited);
  expect(subjectPage([site], "https://stripe.com", [])).toBe(site);
  // A page the run read without a frame is never the picture.
  expect(subjectPage([read("https://stripe.com/", false)], "https://stripe.com", [])).toBe(
    undefined,
  );
});

test("a finding about the subject lends its sentence to the source it cites", () => {
  const execution = structuredClone(projection.executions[0]!);
  execution.user_artifacts = [];
  execution.provider_evidence = [
    {
      id: "record",
      node: "node",
      attempt: "attempt",
      evidence: {
        version: 1,
        provider: "open_ai",
        model: "m",
        response_model: "m",
        response_id: "r",
        search_call_id: "s",
        answer: "",
        actual_input_tokens: 0,
        actual_output_tokens: 0,
        citations: [
          { url: "https://stripe.com/pricing", title: "Pricing", start_index: 0, end_index: 1 },
        ],
      },
    },
  ];
  execution.artifacts = [
    {
      ...execution.artifacts[0]!,
      id: "findings",
      evidence: [{ extraction_id: "record", source_id: 1 }],
      data: {
        kind: "findings",
        subjects: [{ name: "Stripe" }, { name: "Adyen" }],
        items: [
          { claim: "Adyen is Dutch.", subject: 1, evidence: [0], confidence: "supported" },
          { claim: "Stripe charges 2.9%.", subject: 0, evidence: [0], confidence: "supported" },
        ],
      },
    },
  ];
  const detail = subjectDetail(execution, { name: "Stripe" });
  expect(detail.sources.map((source) => source.url)).toEqual(["https://stripe.com/pricing"]);
  expect(Object.values(detail.quotes)).toEqual(["Stripe charges 2.9%."]);
});
