import type {
  WorkArtifactV1,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkRuntimeProjection,
  WorkStepFact,
} from "$shared/ipc/bindings";
import type { BoardScene } from "./board-fixtures";

type Data = WorkArtifactV1["data"];
type Stage = "done" | "working" | "handoff";

const PROFILE = "01J8Z7V6Q5N4M3K2H1G0F9E8D7";
const ROOT = "/Users/ada/Code/zephium";
const LIMITS = {
  model_tokens: 400_000,
  cost_micro_usd: 2_000_000,
  operations: 96,
  timeout_seconds: 1800,
  max_workers: 4,
};
const ISSUE =
  "`--allow-forking=false` not interpreted correctly if forking disabled at organization level";

const FAILED_RUN = `   Compiling zephium-core v0.1.0
    Finished test [unoptimized + debuginfo] target(s) in 4.12s
     Running unittests src/lib.rs
running 43 tests
test work::duration::tests::rounds_to_the_minute ... FAILED
test work::duration::tests::parses_hours ... ok

failures:

---- work::duration::tests::rounds_to_the_minute stdout ----
thread 'work::duration::tests::rounds_to_the_minute' panicked at src/work/duration.rs:88:9:
assertion \`left == right\` failed
  left: 2
 right: 3

test result: FAILED. 42 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out`;
const PASSED_RUN = `running 43 tests
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s`;
const LIVE_RUN = `   Compiling zephium-core v0.1.0 (/Users/ada/Code/zephium/crates/zephium-core)
   Compiling zephium-store v0.1.0 (/Users/ada/Code/zephium/crates/zephium-store)`;
const CODEX_STREAM = [
  { type: "thread.started", thread_id: "t" },
  { type: "item.completed", item: { type: "agent_message", text: "Reading the duration module." } },
  {
    type: "item.started",
    item: {
      type: "file_change",
      changes: [{ path: `${ROOT}/crates/zephium-core/src/work/duration.rs` }],
    },
  },
]
  .map((event) => JSON.stringify(event))
  .join("\n");

function step(
  id: string,
  part: string,
  kind: WorkStepFact["kind"],
  status: WorkStepFact["status"] = "succeeded",
  extra: Partial<WorkStepFact> = {},
): WorkStepFact {
  return { id, turn: 1, kind, status, part, ...extra } as WorkStepFact;
}

/** "Fix the bug in issue #10000", as the lead records it: GitHub and Code parts, then a change. */
export function fixScene(stage: Stage): BoardScene {
  const id = `fix-${stage}`;
  const done = stage === "done";
  const policy = (asks: boolean) =>
    ({
      policy: {
        class: asks ? "write" : "read",
        reason: asks ? "project_execution" : "inspection",
        scope: "none",
        root: ROOT,
      },
    }) as NonNullable<WorkStepFact["local"]>;
  const github: WorkStepFact[] = [
    step(
      "gh-1",
      "github",
      { kind: "read", url: "https://github.com/cli/cli/issues/10000" },
      "succeeded",
      {
        note: `Read issue #10000 · ${ISSUE}`,
      },
    ),
    step("gh-2", "github", { kind: "read", url: "https://github.com/cli/cli/pulls" }, "succeeded", {
      note: "Listed 4 pull requests",
    }),
    step(
      "gh-3",
      "github",
      { kind: "read", url: "https://github.com/cli/cli/pull/14543" },
      "succeeded",
      {
        note: "Read checks on #14543 · 1 failing",
      },
    ),
    ...(stage === "working"
      ? [
          step(
            "gh-4",
            "github",
            {
              kind: "confirm",
              confirm: {
                site: "github.com",
                category: "communication",
                headline: "Comment on #10000 as ada?",
                action: "post the comment",
                text: "Fixed in #14544.",
                facts: [],
              },
            } as WorkStepFact["kind"],
            "running",
          ),
        ]
      : []),
  ];
  const code: WorkStepFact[] = [
    step("c-1", "code", {
      kind: "search_files",
      path: ROOT,
      query: "**/duration*.rs",
      glob: "**/duration*.rs",
      regex: null,
    }),
    step("c-2", "code", {
      kind: "search_files",
      path: ROOT,
      query: "fn minutes",
      glob: null,
      regex: true,
    }),
    step("c-3", "code", {
      kind: "read_file",
      path: `${ROOT}/crates/zephium-core/src/work/duration.rs`,
      offset: 1,
      limit: 400,
    }),
    step("c-4", "code", {
      kind: "read_file",
      path: `${ROOT}/crates/zephium-core/src/work/tests.rs`,
      offset: 60,
      limit: 80,
    }),
    step(
      "c-5",
      "code",
      {
        kind: "run_command",
        cwd: ROOT,
        command: "cargo test -p zephium-core duration",
        timeout_secs: 120,
      },
      "failed",
      { evidence: "rec-1", local: policy(true), note: "1 failed" },
    ),
    step(
      "c-6",
      "code",
      {
        kind: "edit_file",
        path: `${ROOT}/crates/zephium-core/src/work/duration.rs`,
        old: "seconds / 60",
        new: "(seconds + 30) / 60",
        decision: stage === "working" ? null : true,
      },
      stage === "working" ? "running" : "succeeded",
      {
        local: {
          proposal:
            "@@ -86,3 +86,3 @@\n     let seconds = total.as_secs();\n-    let minutes = seconds / 60;\n+    let minutes = (seconds + 30) / 60;\n",
        } as NonNullable<WorkStepFact["local"]>,
      },
    ),
  ];
  if (stage !== "working")
    code.push(
      step(
        "c-7",
        "code",
        {
          kind: "write_file",
          path: `${ROOT}/crates/zephium-core/src/work/tests.rs`,
          content: "",
          decision: true,
        },
        "succeeded",
        {
          local: {
            proposal:
              "@@ -71,0 +71,6 @@\n+#[test]\n+fn rounds_half_a_minute_up() {\n+    assert_eq!(minutes(Duration::from_secs(90)), 2);\n+}\n+\n",
          } as NonNullable<WorkStepFact["local"]>,
        },
      ),
    );
  if (stage === "handoff")
    code.push(
      step(
        "c-8",
        "code",
        {
          kind: "run_command",
          cwd: ROOT,
          command: `codex exec --json --sandbox workspace-write --skip-git-repo-check --ephemeral --color never -C '${ROOT}' 'Add rounding to every duration display'`,
          timeout_secs: 600,
        },
        "running",
        {
          local: {
            ...policy(true),
            output: {
              text: CODEX_STREAM,
              bytes: CODEX_STREAM.length,
              truncated: false,
              elapsed_ms: 48_000,
            },
          } as NonNullable<WorkStepFact["local"]>,
        },
      ),
    );
  else
    code.push(
      step(
        "c-8",
        "code",
        {
          kind: "run_command",
          cwd: ROOT,
          command: "cargo test -p zephium-core duration",
          timeout_secs: 120,
        },
        done ? "succeeded" : "running",
        done
          ? { evidence: "rec-2", local: policy(true), note: "43 passed" }
          : {
              local: {
                ...policy(true),
                output: {
                  text: LIVE_RUN,
                  bytes: LIVE_RUN.length,
                  truncated: false,
                  elapsed_ms: 6_400,
                },
              } as NonNullable<WorkStepFact["local"]>,
            },
      ),
    );
  const record = (rid: string, text: string, exit: number, ms: number) => ({
    id: rid,
    node: "agent",
    attempt: "attempt",
    command: {
      cwd: ROOT,
      command: "cargo test -p zephium-core duration",
      exit,
      signal: null,
      elapsed_ms: ms,
      bytes: text.length,
      digest: "0".repeat(64),
      text,
      truncated: false,
    },
  });
  const base = {
    version: 1,
    execution: id,
    node: "agent",
    attempt: "attempt",
    evidence: [],
    review: "mechanical",
    presentation: "automatic",
  };
  const object = (aid: string, title: string, data: Data, part?: string): WorkArtifactV1 =>
    ({ ...base, id: aid, output: title, title, data, ...(part ? { part } : {}) }) as WorkArtifactV1;
  const made: WorkArtifactV1[] = done
    ? [
        object("fix-reply", "", {
          kind: "reply",
          headline: "Durations now round to the nearest minute",
          text: "`minutes` truncated 90 seconds to 1; it now rounds half a minute up. A test covers it and the whole suite passes.",
          figures: [],
          points: [],
        } as Data),
        object(
          "fix-diff",
          "duration.rs",
          {
            kind: "diff",
            path: "crates/zephium-core/src/work/duration.rs",
            language: "rust",
            summary: "Round to the nearest minute instead of down",
            hunks: [
              {
                old_start: 84,
                new_start: 84,
                lines: [
                  { op: "ctx", text: "pub fn minutes(total: Duration) -> u64 {" },
                  { op: "ctx", text: "    let seconds = total.as_secs();" },
                  { op: "del", text: "    let minutes = seconds / 60;" },
                  { op: "add", text: "    let minutes = (seconds + 30) / 60;" },
                  { op: "ctx", text: "    minutes" },
                  { op: "ctx", text: "}" },
                ],
              },
            ],
          } as Data,
          "code",
        ),
      ]
    : [];
  const run = {
    id,
    approved_revision: "1",
    status: done ? "completed" : "running",
    attempts: [],
    spec: {
      plan_revision: "1",
      request: "Fix the bug in cli/cli issue #10000",
      limits: LIMITS,
      nodes: [
        {
          node: "agent",
          parent: null,
          capability: { kind: "agent", grant: { folders: [ROOT] } },
          limits: LIMITS,
        },
      ],
    },
    artifacts: made,
    provider_evidence: [],
    user_artifacts: [],
    command_evidence: [
      record("rec-1", FAILED_RUN, 101, 4_830),
      ...(done ? [record("rec-2", PASSED_RUN, 0, 3_210)] : []),
    ],
    steps: [...github, ...code],
    parts: [
      {
        id: "github",
        title: "GitHub",
        helper: "connection",
        service: { connection: "github" },
        goal: "Read issue #10000 and the pull requests that touch it",
        state: done || stage === "handoff" ? "done" : "waiting",
        started_ms: "1790620000000",
        ...(done || stage === "handoff" ? { ended_ms: "1790620020000" } : {}),
        summary: "Issue #10000",
      },
      {
        id: "code",
        title: "Code",
        helper: "computer",
        goal: "Find why durations truncate and fix it with a test",
        state: done ? "done" : "running",
        started_ms: "1790620020000",
        ...(done ? { ended_ms: "1790620200000", summary: "2 files changed · tests pass" } : {}),
      },
    ],
  } as unknown as WorkExecutionFact;
  const work = `${id}-work`;
  const projection = {
    version: 1,
    interrupted: [],
    executions: [run],
    work: {
      schema_version: 2,
      profile: PROFILE,
      id: work,
      revision: "1",
      lifecycle: "active",
      objective: "Fix the bug in cli/cli issue #10000",
      objective_revision: "1",
      context_revision: "1",
      objective_author: "user",
      questions: [],
      status: done ? "plan_ready" : "running",
      plan: null,
    },
  } as unknown as WorkRuntimeProjection;
  const snapshot = {
    version: 1,
    profile: PROFILE,
    id: `${id}-canvas`,
    space: "space",
    title: id,
    revision: "1",
    lifecycle: "active",
    areas: [],
    view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
    elements: [
      { id: `${id}-request`, area: null, reference: { kind: "objective", objective: work } },
      ...made.map((artifact) => ({
        id: `el-${artifact.id}`,
        area: null,
        reference: { kind: "artifact", objective: work, execution: id, artifact: artifact.id },
      })),
    ],
  } as unknown as WorkEnvironmentSnapshot;
  return {
    name: id,
    snapshot,
    objectives: new Map([[work, projection]]),
    pictures: new Map(),
    pages: [],
  };
}
