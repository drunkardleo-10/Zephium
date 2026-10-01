import type {
  ResourceCall_Deserialize,
  ResourceDraft_Deserialize,
  ResourceDraft_Serialize,
  ResourceReply_Serialize,
  ResourceRecord_Serialize,
  ResourceSummary,
  TaskCounts,
  TaskField,
  TaskList,
} from "$shared/ipc/bindings";

type TaskContent = Extract<ResourceDraft_Serialize["content"], { kind: "task" }>;

/** The day a task is next answerable for, as native places it. */
const dayDue = (task: TaskContent) => {
  const deadline = task.details.deadline ?? null;
  if (deadline === null) return task.due_date;
  return task.due_date === null || deadline < task.due_date ? deadline : task.due_date;
};

function holds(draft: ResourceDraft_Serialize, field: TaskField): boolean {
  const task = draft.content as TaskContent;
  const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
  switch (field.field) {
    case "title":
      return draft.title === field.value;
    case "description":
      return task.description === field.value;
    case "status":
      return task.status === field.value;
    case "schedule":
      return task.due_date === field.date && (task.due_time ?? null) === field.time;
    case "deadline":
      return (task.details.deadline ?? null) === field.date;
    case "duration":
      return (task.details.duration ?? null) === field.minutes;
    case "organization":
      return task.details.list === field.list && task.details.inbox === field.inbox;
    case "priority":
      return task.details.priority === field.value;
    case "steps":
      return same(task.details.steps, field.value);
    case "pinned":
      return draft.pinned === field.value;
    case "position":
      return (task.sort_key ?? null) === field.sort_key;
  }
}

function write(draft: ResourceDraft_Serialize, field: TaskField): ResourceDraft_Serialize {
  const task = draft.content as TaskContent;
  const details = (patch: Partial<TaskContent["details"]>) => ({
    ...draft,
    content: { ...task, details: { ...task.details, ...patch } },
  });
  switch (field.field) {
    case "title":
      return { ...draft, title: field.value };
    case "description":
      return { ...draft, content: { ...task, description: field.value } };
    case "status":
      return {
        ...draft,
        content: {
          ...task,
          status: field.value,
          completed: field.value === "done",
          details: {
            ...task.details,
            completed_at:
              field.value !== "done" ? null : task.completed ? task.details.completed_at : "200",
          },
        },
      };
    case "schedule":
      return { ...draft, content: { ...task, due_date: field.date, due_time: field.time } };
    case "deadline":
      return details({ deadline: field.date });
    case "duration":
      return details({ duration: field.minutes });
    case "organization":
      return details({ list: field.list, inbox: field.inbox });
    case "priority":
      return details({ priority: field.value });
    case "steps":
      return details({ steps: field.value });
    case "pinned":
      return { ...draft, pinned: field.value };
    case "position":
      return { ...draft, content: { ...task, sort_key: field.sort_key } };
  }
}
/** Mirrors the defaults Rust applies to task fields a stored body may predate, so
 *  a draft written without them lands exactly as native would store it. */
const adoptDraft = (draft: ResourceDraft_Deserialize): ResourceDraft_Serialize =>
  draft.content.kind === "task"
    ? {
        ...draft,
        content: {
          ...draft.content,
          details: {
            list: null,
            inbox: false,
            priority: "none" as const,
            steps: [],
            completed_at: null,
            deadline: null,
            duration: null,
            ...draft.content.details,
          },
          status: draft.content.status ?? (draft.content.completed ? "done" : "open"),
          assignee: draft.content.assignee ?? "user",
          origin: draft.content.origin ?? "user",
        },
      }
    : { ...draft, content: draft.content };

/** Component-test transport only. Rust persistence is verified separately against real SQLite. */
export function resourceTestServer(profile: string) {
  const records = new Map<string, ResourceRecord_Serialize>();
  const lists = new Map<string, TaskList>();
  const receipts = new Map<string, ResourceReply_Serialize>();
  let next = 1;
  const summary = (record: ResourceRecord_Serialize): ResourceSummary => {
    const task = record.draft.content.kind === "task" ? record.draft.content : null;
    return {
      id: record.id,
      revision: record.revision,
      title: record.draft.title,
      pinned: record.draft.pinned,
      updated_at: record.updated_at,
      completed: task?.completed ?? null,
      due_date: task?.due_date ?? null,
      due_time: task?.due_time ?? null,
      status: task?.status ?? null,
      assignee: task?.assignee ?? null,
      origin: task?.origin ?? null,
      context: task?.context ?? null,
      sort_key: task?.sort_key ?? null,
      work: task?.work ?? null,
    };
  };
  const tasks = () =>
    [...records.values()].filter((record) => record.draft.content.kind === "task");
  const active = () =>
    tasks().filter(
      (record) =>
        !record.trashed && record.draft.content.kind === "task" && !record.draft.content.completed,
    );
  const activeLists = () =>
    [...lists.values()]
      .filter((list) => !list.deleted)
      .map((list) => ({
        ...list,
        count: active().filter(
          (record) =>
            record.draft.content.kind === "task" && record.draft.content.details.list === list.id,
        ).length,
      }));
  const counts = (today: string): TaskCounts => {
    const open = active();
    const due = (record: ResourceRecord_Serialize) => dayDue(record.draft.content as TaskContent);
    return {
      inbox: open.filter((r) => (r.draft.content as TaskContent).details.inbox).length,
      today: open.filter((r) => due(r) !== null && due(r)! <= today).length,
      overdue: open.filter((r) => due(r) !== null && due(r)! < today).length,
      upcoming: open.filter((r) => due(r) !== null && due(r)! > today).length,
      all: open.length,
      completed: tasks().filter((r) => !r.trashed && (r.draft.content as TaskContent).completed)
        .length,
      trash: tasks().filter((r) => r.trashed).length,
    };
  };
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
    if (call.kind === "list_tasks") {
      const query = call.query;
      const due = (record: ResourceRecord_Serialize) =>
        record.draft.content.kind === "task" ? dayDue(record.draft.content) : null;
      const items = tasks()
        .filter((record) => {
          const task = record.draft.content;
          if (task.kind !== "task" || record.trashed !== (query.view === "trash")) return false;
          if (
            !`${record.draft.title}\n${task.description}`
              .toLowerCase()
              .includes(query.search.toLowerCase())
          )
            return false;
          if (query.list && task.details.list !== query.list) return false;
          if (query.view === "inbox") return task.details.inbox === true && !task.completed;
          const day = dayDue(task);
          if (query.view === "today") return !task.completed && day !== null && day <= query.today;
          if (query.view === "upcoming")
            return !task.completed && day !== null && day > query.today;
          if (query.view === "completed") return task.completed;
          return true;
        })
        .map(summary)
        .sort((a, b) => {
          const group = (row: ResourceSummary) => {
            const day = due(records.get(row.id)!);
            return row.completed
              ? 5
              : day === null
                ? 4
                : day < query.today
                  ? 0
                  : day === query.today
                    ? 1
                    : 3;
          };
          const ranks = { blocked: 0, active: 1, open: 2, done: 3 };
          return (
            group(a) - group(b) ||
            ranks[a.status ?? "open"] - ranks[b.status ?? "open"] ||
            Number(b.pinned) - Number(a.pinned) ||
            (a.sort_key ?? "~").localeCompare(b.sort_key ?? "~") ||
            (a.due_date ?? "9999-12-31").localeCompare(b.due_date ?? "9999-12-31") ||
            (a.due_time ?? "24:00").localeCompare(b.due_time ?? "24:00") ||
            a.id.localeCompare(b.id)
          );
        });
      const start = query.after ? items.findIndex((row) => row.id === query.after) + 1 : 0;
      const page = items.slice(start, start + query.limit);
      return response({
        kind: "task_page",
        lists: activeLists(),
        metadata: page.map((row) => {
          const record = records.get(row.id)!;
          const details = record.draft.content.kind === "task" ? record.draft.content.details : {};
          return {
            id: row.id,
            list: details.list ?? null,
            inbox: details.inbox ?? false,
            priority: details.priority ?? "none",
            steps: details.steps?.length ?? 0,
            steps_done: details.steps?.filter((step) => step.completed).length ?? 0,
            completed_at: details.completed_at ?? null,
            deadline: details.deadline ?? null,
            duration: details.duration ?? null,
          };
        }),
        items: page,
        next: start + query.limit < items.length ? page.at(-1)!.id : null,
        counts: counts(query.today),
      });
    }
    if (call.kind === "task_overview")
      return response({ kind: "task_overview", lists: activeLists(), counts: counts(call.today) });
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
    if (
      intent.kind === "create_task_list" ||
      intent.kind === "rename_task_list" ||
      intent.kind === "delete_task_list"
    ) {
      let list: TaskList;
      if (intent.kind === "create_task_list") {
        while (
          lists.has(String(next).padStart(26, "0")) ||
          records.has(String(next).padStart(26, "0"))
        )
          next++;
        list = {
          id: String(next++).padStart(26, "0"),
          title: intent.title,
          revision: "1",
          count: 0,
          deleted: false,
        };
      } else {
        const current = lists.get(intent.id);
        if (!current) return response({ kind: "error", error: "not_found" });
        if (current.revision !== intent.expected_revision)
          return response({ kind: "error", error: "conflict" });
        list = {
          ...current,
          revision: String(BigInt(current.revision) + 1n),
          title: intent.kind === "rename_task_list" ? intent.title : current.title,
          deleted: intent.kind === "delete_task_list",
        };
        if (list.deleted)
          for (const [id, record] of records) {
            if (
              record.draft.content.kind === "task" &&
              record.draft.content.details.list === list.id
            )
              records.set(id, {
                ...record,
                revision: String(BigInt(record.revision) + 1n),
                draft: {
                  ...record.draft,
                  content: {
                    ...record.draft.content,
                    details: { ...record.draft.content.details, list: null, inbox: true },
                  },
                },
              });
          }
      }
      lists.set(list.id, list);
      const reply = response({
        kind: "task_list_applied",
        request_id: call.command.request_id,
        list: structuredClone(list),
      });
      receipts.set(call.command.request_id, reply);
      return reply;
    }
    let record: ResourceRecord_Serialize;
    if (intent.kind === "create") {
      while (records.has(String(next).padStart(26, "0"))) next++;
      record = {
        id: String(next++).padStart(26, "0"),
        revision: "1",
        created_at: "100",
        updated_at: "100",
        trashed: false,
        draft: adoptDraft(structuredClone(intent.draft)),
      };
    } else if (intent.kind === "update_task") {
      const current = records.get(intent.id);
      if (!current) return response({ kind: "error", error: "not_found" });
      if (current.trashed || !(intent.expect ?? []).every((field) => holds(current.draft, field)))
        return response({ kind: "error", error: "conflict" });
      record = {
        ...current,
        revision: String(BigInt(current.revision) + 1n),
        draft: intent.set.reduce(write, current.draft),
      };
    } else {
      if (intent.kind === "preserve_artifact")
        return response({ kind: "error", error: "unavailable" });
      const current = records.get(intent.id);
      if (!current) return response({ kind: "error", error: "not_found" });
      if (current.revision !== intent.expected_revision)
        return response({ kind: "error", error: "conflict" });
      record = {
        ...current,
        revision: String(BigInt(current.revision) + 1n),
        draft:
          intent.kind === "replace" ? adoptDraft(structuredClone(intent.draft)) : current.draft,
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
  return { records, lists, call };
}
