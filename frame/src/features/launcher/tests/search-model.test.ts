import { describe, expect, it } from "vitest";
import { sameSearch, resultIdentity, keepSelection } from "../lib/search-model";
import type { SearchContext, SearchResult } from "$shared/ipc/bindings";
const context = (session_id = "one", request_id = "one"): SearchContext => ({
  window_id: "window",
  session_id,
  request_id,
  profile_id: "p",
  space_id: "s",
});
const tab = (id: string, title = id): SearchResult => ({
  kind: "tab",
  title,
  detail: "example.com",
  favicon: null,
  action: { type: "ActivateTab", id },
});
describe("search identity", () => {
  it("does not admit an ABA query from another session or profile", () => {
    expect(sameSearch(context(), context())).toBe(true);
    expect(sameSearch(context(), context("two"))).toBe(false);
    expect(sameSearch(context(), { ...context(), profile_id: "other" })).toBe(false);
    expect(sameSearch(context(), context("one", "two"))).toBe(false);
  });
  it("preserves selection across title updates and result insertions", () => {
    const selected = resultIdentity(tab("a"));
    expect(keepSelection(selected, [tab("b"), tab("a", "Changed title")])).toBe(selected);
    expect(keepSelection(selected, [tab("b")])).toBe(resultIdentity(tab("b")));
  });
});
