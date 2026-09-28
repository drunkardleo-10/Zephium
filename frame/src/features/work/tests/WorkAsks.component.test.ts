import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { WorkSession } from "$domain/work";
import { WorkHumanSession } from "$domain/work-human";
import AskCard from "../components/asks/AskCard.svelte";
import RunAsks from "../components/asks/RunAsks.svelte";
import { partAsks, runActions } from "../components/asks/actions";
import { asksOf } from "../components/asks/asks";
import { projection } from "./environment-fixtures";
import * as f from "./ask-fixtures";

const native = vi.hoisted(() => ({ operation: vi.fn(), human: vi.fn() }));
vi.mock("$shared/ipc/bindings", async () => {
  const { mockBindings } = await import("$shared/testing/bindings");
  return mockBindings({ workOperation: native.operation, workHumanContinue: native.human });
});

function sessionWith(...steps: Parameters<typeof f.runWith>[0]) {
  const session = new WorkSession("profile");
  session.selected = "objective";
  const execution = f.runWith(steps);
  session.projection = {
    ...structuredClone(projection),
    executions: [execution],
    owners: [{ execution: execution.id, owner: "owner" }],
  };
  return { session, execution };
}

test("Confirm approves, allows for the run, or declines through ApproveStep on its own step", async () => {
  for (const [button, shape] of [
    ["Send", { approve: true }],
    ["Not now", { approve: false }],
  ] as const) {
    const { session, execution } = sessionWith(f.slackSend);
    const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
    const [ask] = asksOf(execution);
    const screen = await render(AskCard, { ask: ask!, actions: runActions(session, execution.id) });
    await screen.getByRole("button", { name: button, exact: true }).click();
    expect(execute).toHaveBeenCalledExactlyOnceWith({
      kind: "approve_step",
      execution: execution.id,
      step: "confirm-slack",
      ...shape,
    });
    await screen.unmount();
    session.dispose();
  }
  const { session, execution } = sessionWith(f.notionEdit);
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  const screen = await render(AskCard, {
    ask: asksOf(execution)[0]!,
    actions: runActions(session, execution.id),
  });
  await screen.getByRole("button", { name: "Allow for this run", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "approve_step",
    execution: execution.id,
    step: "confirm-notion",
    approve: true,
    for_run: true,
  });
  await screen.unmount();
  session.dispose();
});

test("a decision is sent once and a refused one says so without leaving the card", async () => {
  const { session, execution } = sessionWith(f.airbnbBook);
  let settle: (sent: boolean) => void = () => {};
  const execute = vi
    .spyOn(session, "execute")
    .mockImplementation(() => new Promise((done) => (settle = done)));
  const screen = await render(AskCard, {
    ask: asksOf(execution)[0]!,
    actions: runActions(session, execution.id),
  });
  const book = screen.getByRole("button", { name: "Request to book", exact: true });
  await book.click();
  await expect.element(screen.getByRole("button", { name: "Not now", exact: true })).toBeDisabled();
  settle(false);
  await expect.element(screen.getByRole("alert")).toHaveTextContent("That didn't go through");
  expect(execute).toHaveBeenCalledTimes(1);
  await expect.element(book).toBeEnabled();
  await screen.unmount();
  session.dispose();
});

test("a decided Confirm stays as one line of history", async () => {
  const { session, execution } = sessionWith(f.decided(f.slackSend, "approved", "succeeded"));
  const screen = await render(AskCard, {
    ask: asksOf(execution)[0]!,
    actions: runActions(session, execution.id),
  });
  await expect.element(screen.getByRole("status")).toHaveTextContent("SentSend to #design");
  expect(screen.container.querySelector("button")).toBeNull();
  await screen.unmount();
  session.dispose();
});

test("entry, context and connection questions answer with Rust's own option words", async () => {
  for (const [step, button, answer] of [
    [f.slackEntry, "Always", "Always for Slack"],
    [f.slackEntry, "Allow", "Allow"],
    [f.historyAsk, "Not now", "Not now"],
    [f.githubAsk, "Use the website instead", "Use the website instead"],
    [f.budgetAsk, "$3,000 – $5,000", "$3,000 – $5,000"],
  ] as const) {
    const { session, execution } = sessionWith(f.slackTask, step);
    const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
    const ask = asksOf(execution).find((entry) => entry.step === step.id)!;
    const screen = await render(AskCard, { ask, actions: runActions(session, execution.id) });
    await screen.getByRole("button", { name: button, exact: true }).click();
    expect(execute).toHaveBeenCalledExactlyOnceWith({
      kind: "answer_step",
      execution: execution.id,
      step: step.id,
      answer,
    });
    await screen.unmount();
    session.dispose();
  }
});

test("a sign-in wall opens the held page, and 'I've signed in' carries it on", async () => {
  const { session, execution } = sessionWith(f.notionTask);
  const human = new WorkHumanSession("profile");
  native.human.mockResolvedValue({
    version: 1,
    profile: "profile",
    work: "objective",
    accepted: true,
    pages: [],
    error: null,
  });
  const open = vi.fn();
  const [ask] = asksOf(execution, [], [f.notionWall]);
  const screen = await render(AskCard, {
    ask: ask!,
    actions: runActions(session, execution.id, { session: human, work: "objective" }, open),
  });
  await expect
    .element(screen.getByRole("heading", { name: "Sign in to Notion to continue" }))
    .toBeVisible();
  await screen.getByRole("button", { name: "Sign in", exact: true }).click();
  expect(open).toHaveBeenCalledExactlyOnceWith("read-notion");
  await screen.getByRole("button", { name: "I've signed in", exact: true }).click();
  expect(native.human).toHaveBeenCalledExactlyOnceWith(
    "profile",
    "objective",
    f.notionWall.id,
    "anonymous",
  );
  await screen.unmount();
  session.dispose();
});

test("the island holds a live run's open asks, newest first, and the rows get theirs by part", async () => {
  const { session, execution } = sessionWith(
    f.slackTask,
    f.answered(f.slackEntry, "Allow"),
    f.slackSend,
    f.airbnbBook,
  );
  const execute = vi.spyOn(session, "execute").mockResolvedValue(true);
  const screen = await render(RunAsks, { session });
  const cards = screen.container.querySelectorAll<HTMLElement>("[data-ask]");
  expect([...cards].map((card) => card.dataset.step)).toEqual(["confirm-airbnb", "confirm-slack"]);
  await screen.getByRole("button", { name: "Send", exact: true }).click();
  expect(execute).toHaveBeenCalledExactlyOnceWith({
    kind: "approve_step",
    execution: execution.id,
    step: "confirm-slack",
    approve: true,
  });
  expect(partAsks(session).map((ask) => [ask.part, ask.view])).toEqual([
    ["airbnb.co.uk", "AskCard"],
    ["slack.com", "AskCard"],
  ]);
  await screen.unmount();
  session.dispose();
});
