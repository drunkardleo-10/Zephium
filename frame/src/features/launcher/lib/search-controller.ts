import type { SearchContext, SearchResults, SearchResult } from "$shared/ipc/bindings";
import { sameSearch } from "./search-model";
export type SearchSnapshot = {
  results: SearchResult[];
  pending: boolean;
  error: "none" | "failed" | "too_long";
};
export function createSearchController(options: {
  owner: Omit<SearchContext, "request_id"> | null;
  send: (query: string, id: string) => Promise<boolean>;
  update: (snapshot: SearchSnapshot) => void;
  id?: () => string;
}) {
  let query = "";
  let composing = false;
  let disposed = false;
  let active: SearchContext | null = null;
  let received = false;
  let debounce: ReturnType<typeof setTimeout> | undefined;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  const publish = (snapshot: SearchSnapshot) => {
    if (!disposed) options.update(snapshot);
  };
  const clear = () => {
    clearTimeout(debounce);
    clearTimeout(deadline);
  };
  function fail(request: SearchContext) {
    if (disposed || !sameSearch(active, request) || received) return;
    clearTimeout(deadline);
    active = null;
    publish({ results: [], pending: false, error: "failed" });
  }
  async function send() {
    if (disposed || composing) return;
    if (new TextEncoder().encode(query).byteLength > 2048) {
      publish({ results: [], pending: false, error: "too_long" });
      return;
    }
    if (!options.owner) {
      publish({ results: [], pending: false, error: "none" });
      return;
    }
    const request = { ...options.owner, request_id: (options.id ?? (() => crypto.randomUUID()))() };
    active = request;
    received = false;
    publish({ results: [], pending: true, error: "none" });
    deadline = setTimeout(() => fail(request), 3000);
    try {
      if (!(await options.send(query, request.request_id))) fail(request);
    } catch {
      fail(request);
    }
  }
  function change(value: string) {
    query = value;
    clear();
    active = null;
    received = false;
    publish({ results: [], pending: false, error: "none" });
    if (!composing && !disposed) debounce = setTimeout(() => void send(), 60);
  }
  return {
    start: () => void send(),
    change,
    compositionStart() {
      composing = true;
      clear();
      active = null;
    },
    compositionEnd(value: string) {
      composing = false;
      change(value);
    },
    context: () => active,
    receive(result: SearchResults) {
      if (disposed || !sameSearch(active, result.context) || result.query !== query) return;
      clearTimeout(deadline);
      received = true;
      publish({ results: result.results.slice(0, 10), pending: false, error: "none" });
    },
    dispose() {
      disposed = true;
      clear();
      active = null;
    },
  };
}
