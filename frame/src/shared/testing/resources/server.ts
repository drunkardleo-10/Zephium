import type {
  ResourceCall_Deserialize,
  ResourceReply_Serialize,
  ResourceRecord_Serialize,
  ResourceSummary,
} from "$shared/ipc/bindings";
/** Component-test transport only. Rust persistence is verified separately against real SQLite. */
export function resourceTestServer(profile: string) {
  const records = new Map<string, ResourceRecord_Serialize>();
  const receipts = new Map<string, ResourceReply_Serialize>();
  let next = 1;
  const summary = (record: ResourceRecord_Serialize): ResourceSummary => ({
    id: record.id,
    revision: record.revision,
    title: record.draft.title,
    pinned: record.draft.pinned,
    updated_at: record.updated_at,
    completed: record.draft.content.kind === "task" ? record.draft.content.completed : null,
    due_date: record.draft.content.kind === "task" ? record.draft.content.due_date : null,
  });
  async function call(
    expected: string,
    call: ResourceCall_Deserialize,
  ): Promise<ResourceReply_Serialize> {
    const response = (response: ResourceReply_Serialize["response"]): ResourceReply_Serialize => ({
      profile,
      response,
    });
    if (expected !== profile)
      return { profile: null, response: { kind: "error", error: "unavailable" } };
    if (call.kind === "acknowledge") return response({ kind: "acknowledged" });
    if (call.kind === "get") {
      const record = records.get(call.id);
      return response(
        record
          ? { kind: "record", record: structuredClone(record) }
          : { kind: "error", error: "not_found" },
      );
    }
    if (call.kind === "resolve_notes")
      return response({
        kind: "page",
        items: call.ids.flatMap((id) => {
          const record = records.get(id);
          return record && !record.trashed ? [summary(record)] : [];
        }),
        next: null,
      });
    if (call.kind === "list")
      return response({
        kind: "page",
        items: [...records.values()]
          .filter(
            (record) =>
              record.draft.content.kind === call.query.kind &&
              record.trashed === call.query.trashed &&
              record.draft.title.toLowerCase().includes(call.query.search.toLowerCase()) &&
              (call.query.completed == null ||
                (record.draft.content.kind === "task" &&
                  record.draft.content.completed === call.query.completed)),
          )
          .map(summary),
        next: null,
      });
    const cached = receipts.get(call.command.request_id);
    if (cached) return structuredClone(cached);
    const intent = call.command.intent;
    let record: ResourceRecord_Serialize;
    if (intent.kind === "create")
      record = {
        id: String(next++).padStart(26, "0"),
        revision: "1",
        created_at: "100",
        updated_at: "100",
        trashed: false,
        draft: structuredClone(intent.draft),
      };
    else {
      if (intent.kind === "preserve_artifact")
        return response({ kind: "error", error: "unavailable" });
      const current = records.get(intent.id);
      if (!current) return response({ kind: "error", error: "not_found" });
      if (current.revision !== intent.expected_revision)
        return response({ kind: "error", error: "conflict" });
      record = {
        ...current,
        revision: String(BigInt(current.revision) + 1n),
        draft: intent.kind === "replace" ? structuredClone(intent.draft) : current.draft,
        trashed:
          intent.kind === "trash" ? true : intent.kind === "restore" ? false : current.trashed,
      };
    }
    records.set(record.id, record);
    const reply = response({
      kind: "applied",
      request_id: call.command.request_id,
      applied_revision: record.revision,
      record: structuredClone(record),
    });
    receipts.set(call.command.request_id, reply);
    return reply;
  }
  return { records, call };
}
