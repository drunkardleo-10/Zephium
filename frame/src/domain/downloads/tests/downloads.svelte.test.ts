import { beforeEach, expect, test, vi } from "vitest";
import type { DownloadResponse, DownloadView } from "$shared/ipc/bindings";

const host = vi.hoisted(() => ({
  call: vi.fn(),
  listener: null as null | ((event: { payload: { profile: string } }) => void),
}));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ downloadCall: host.call });
});
vi.mock("$shared/ipc/native-events", () => ({
  events: {
    downloadsChanged: {
      listen: (listener: typeof host.listener) => {
        host.listener = listener;
        return Promise.resolve(() => {
          host.listener = null;
        });
      },
    },
  },
}));
const profile = "00000000000000000000000001";
const entry: DownloadView = {
  id: "00000000000000000000000002",
  revision: "00000001",
  created_at: "1",
  filename: "fixture.txt",
  source: "https://example.com",
  state: "receiving",
  received: "10",
  total: "100",
  error: null,
};
function deferred() {
  let resolve!: (response: DownloadResponse) => void;
  return {
    promise: new Promise<DownloadResponse>((done) => {
      resolve = done;
    }),
    resolve: (response: DownloadResponse) => resolve(response),
  };
}
beforeEach(() => {
  vi.resetModules();
  host.call.mockReset();
  host.listener = null;
});

test("the status indicator performs no startup history query and ignores another profile", async () => {
  host.call.mockResolvedValue({ kind: "updates", entries: [entry], removed: [] });
  const { DownloadSession } = await import("../downloads.svelte");
  const session = new DownloadSession(profile);
  await session.start(false);
  expect(host.call).not.toHaveBeenCalled();
  host.listener?.({ payload: { profile: "other" } });
  expect(host.call).not.toHaveBeenCalled();
  host.listener?.({ payload: { profile } });
  await vi.waitFor(() => expect(session.entries).toHaveLength(1));
  expect(host.call).toHaveBeenCalledWith(profile, { kind: "updates" });
  session.stop();
});

test("a late history page cannot roll back newer native progress", async () => {
  const page = deferred();
  host.call.mockImplementation((_profile, call) =>
    call.kind === "list"
      ? page.promise
      : Promise.resolve({
          kind: "updates",
          entries: [{ ...entry, revision: "00000002", received: "80" }],
          removed: [],
        }),
  );
  const { DownloadSession } = await import("../downloads.svelte");
  const session = new DownloadSession(profile);
  const loading = session.reload();
  await session.refresh();
  page.resolve({ kind: "page", entries: [entry], next: null, supported: true });
  await loading;
  expect(session.entries[0]?.received).toBe("80");
});

test("a removal cannot be resurrected by an older in-flight history page", async () => {
  const page = deferred();
  host.call.mockImplementation((_profile, call) =>
    call.kind === "list"
      ? page.promise
      : Promise.resolve({ kind: "updates", entries: [], removed: [entry.id] }),
  );
  const { DownloadSession } = await import("../downloads.svelte");
  const session = new DownloadSession(profile);
  const loading = session.reload();
  await session.refresh();
  page.resolve({ kind: "page", entries: [entry], next: null, supported: true });
  await loading;
  expect(session.entries).toHaveLength(0);
});

test("a stopped profile ignores an outstanding native response", async () => {
  const page = deferred();
  host.call.mockReturnValue(page.promise);
  const { DownloadSession } = await import("../downloads.svelte");
  const session = new DownloadSession(profile);
  const loading = session.reload();
  session.stop();
  page.resolve({ kind: "page", entries: [entry], next: null, supported: true });
  await loading;
  expect(session.entries).toHaveLength(0);
});

test("a download started during a history read survives an older empty page", async () => {
  const page = deferred();
  host.call.mockImplementation((_profile, call) =>
    call.kind === "list"
      ? page.promise
      : Promise.resolve({ kind: "updates", entries: [entry], removed: [] }),
  );
  const { DownloadSession } = await import("../downloads.svelte");
  const session = new DownloadSession(profile);
  const loading = session.reload();
  await session.refresh();
  page.resolve({ kind: "page", entries: [], next: null, supported: true });
  await loading;
  expect(session.entries).toEqual([entry]);
});
