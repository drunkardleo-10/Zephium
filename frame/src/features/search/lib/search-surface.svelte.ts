import { tick } from "svelte";
import type { IconSvgElement } from "@hugeicons/svelte";
import type { PanelState, SearchResult, ToolKind } from "$shared/ipc/bindings";
import { commands } from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { settle } from "$domain/operations";
import { createSearchController, type SearchSnapshot } from "./search-controller";
import {
  completionSuffix,
  completionTarget,
  resultIdentity,
  resultSection,
  type ResultSection,
} from "./search-model";

export type Destination = { kind: ToolKind; label: string; icon: IconSvgElement };

export type SurfaceRow = {
  id: string;
  title: string;
  section: ResultSection;
  result: SearchResult | null;
  tool: ToolKind | null;
  icon: IconSvgElement | null;
};

/** Shared behaviour for both search hosts. The floating launcher and the New
 *  Tab capsule differ only in chrome; selection, coalescing, execution and
 *  cancellation are one implementation. */
export function createSearchSurface(options: {
  /** Set for New Tab, which binds a specific blank tab and anchors its list. */
  tabId: string | null;
  context: PanelState | null;
  destinations: Destination[];
  onTool: (tool: ToolKind) => void;
}) {
  const anchored = options.tabId !== null;

  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let completion = $state<string | null>(null);
  /** The completion currently standing in the field, or null when the field
   *  holds only what was typed. */
  let applied = $state<string | null>(null);
  let answered = $state("");
  let pending = $state(false);
  let settled = $state(false);
  let error = $state<SearchSnapshot["error"]>("none");
  let failed = $state(false);
  let running = $state(false);
  let open = $state(!anchored);
  let selected = $state<string | null>(null);
  /** The user has aimed at a row. Until then the default is row 0, which is
   *  always the action for what they typed. */
  let moved = $state(false);
  let deleting = $state(false);
  /** What the field is actually showing, completion included. Deletion has to
   *  be judged against this: comparing against the typed text made backspace
   *  look broken, because removing the completion leaves the typed text the
   *  same length and the completion was immediately re-applied. */
  let displayed = $state("");
  let composing = $state(false);

  /** Intent expressed before the answer arrived. Resolved against the settled
   *  result set so a click or Enter is never silently dropped, and never runs
   *  an action the newer query no longer offers. `""` means "the first row",
   *  which is the typed action. */
  let queued: string | null = null;

  let disposed = false;
  let list: HTMLElement | undefined;
  let controller: ReturnType<typeof createSearchController> | undefined;

  const matchingTools = $derived(
    query.trim() && !anchored
      ? options.destinations.filter((destination) =>
          destination.label.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
        )
      : [],
  );

  // Native emits results already in section order, so rows keep that order and
  // tool destinations join the Commands run at the end.
  const rows = $derived<SurfaceRow[]>([
    ...results.map((result) => ({
      id: resultIdentity(result),
      title: result.title,
      section: resultSection(result),
      result,
      tool: null,
      icon: null,
    })),
    ...matchingTools.map((destination) => ({
      id: `tool:${destination.kind}`,
      title: destination.label,
      section: "commands" as const,
      result: null,
      tool: destination.kind,
      icon: destination.icon,
    })),
  ]);

  /** The row the field is currently showing the user, when a completion has
   *  been applied. Safari highlights this row, and Enter opens it. */
  const completed = $derived(completionTarget(rows, applied));

  const selectedId = $derived(
    rows.length === 0
      ? null
      : moved && selected && rows.some((row) => row.id === selected)
        ? selected
        : (completed?.id ?? rows[0]!.id),
  );

  async function run(row: SurfaceRow) {
    if (running) return;
    if (row.tool) {
      options.onTool(row.tool);
      return;
    }
    const expected = controller?.context();
    if (!row.result || !expected) return;
    running = true;
    failed = false;
    try {
      const admission = anchored
        ? await settle(commands.newtabRun(row.result.action, expected))
        : await commands.launcherRun(row.result.action, expected);
      if (disposed) return;
      const rejected =
        "accepted" in admission
          ? !admission.accepted
          : admission.outcome === "failed" || admission.outcome === "rejected";
      running = false;
      if (rejected) failed = true;
      else if (anchored) open = false;
    } catch {
      if (!disposed) {
        running = false;
        failed = true;
      }
    }
  }

  /** Runs now when the visible rows answer the current query, otherwise waits
   *  for the answer rather than doing nothing. */
  function activate(row: SurfaceRow) {
    if (settled) void run(row);
    else queued = row.id;
  }

  function resolveQueued() {
    if (queued === null || !settled || running) return;
    const target = queued === "" ? rows[0] : rows.find((row) => row.id === queued);
    queued = null;
    if (target) void run(target);
  }

  async function move(offset: number) {
    if (!rows.length) return;
    const at = rows.findIndex((row) => row.id === selectedId);
    const next = at + offset;
    // No wrap. Past the top the selection returns to the typed action; past
    // the bottom it stays put, so holding a key never cycles the list.
    if (next < 0) {
      moved = false;
      selected = null;
      return;
    }
    if (next >= rows.length) return;
    moved = true;
    selected = rows[next]!.id;
    await tick();
    list?.querySelector<HTMLElement>(`[aria-posinset="${next + 1}"]`)?.scrollIntoView({
      block: "nearest",
    });
  }

  function changed(value: string) {
    deleting = value.length < displayed.length && displayed.startsWith(value);
    displayed = value;
    query = value;
    applied = null;
    if (anchored) open = true;
    queued = null;
    moved = false;
    selected = null;
    failed = false;
    running = false;
    controller?.change(value);
  }

  function submit() {
    if (composing) return;
    const row = rows.find((candidate) => candidate.id === selectedId);
    if (settled && row) {
      void run(row);
      return;
    }
    if (query.trim()) queued = row?.id ?? "";
  }

  /** Appends the offered host and selects the appended part, so the next
   *  keystroke replaces it. Never fights the typist: not while composing, not
   *  after a deletion, and not when the caret sits inside the text. */
  function applyCompletion(input: HTMLInputElement | undefined) {
    if (!input || deleting || composing || !completion) return;
    const suffix = completionSuffix(query, completion);
    if (!suffix) return;
    if (input.value !== query) return;
    if (input.selectionStart !== query.length || input.selectionEnd !== query.length) return;
    input.value = query + suffix;
    input.setSelectionRange(query.length, query.length + suffix.length);
    applied = completion;
    displayed = input.value;
  }

  function dismiss() {
    // First press puts the list away and keeps the query; a second clears it.
    if (anchored && open) {
      open = false;
      queued = null;
      return true;
    }
    if (query) {
      changed("");
      return true;
    }
    return false;
  }

  function focused() {
    if (!open && query) controller?.change(query);
    open = true;
  }

  function blurred() {
    if (!anchored) return;
    open = false;
    queued = null;
    const active = controller?.request();
    controller?.pause();
    if (active) void commands.newtabCancel(active).catch(() => {});
  }

  function retry() {
    failed = false;
    controller?.change(query);
  }

  /** Called from the host's `onMount`; returns its teardown. */
  function mount() {
    const listener = events.searchChanged.listen((event) => controller?.receive(event.payload));
    void listener
      .then(() => {
        if (!disposed) return initialize();
      })
      .catch(() => {
        if (!disposed) failed = true;
      });

    async function initialize() {
      const owner = options.tabId
        ? await commands.newtabSearchContext(options.tabId)
        : options.context?.window_id && options.context.profile_id && options.context.space_id
          ? {
              window_id: options.context.window_id,
              profile_id: options.context.profile_id,
              space_id: options.context.space_id,
              session_id: options.context.session_id,
            }
          : null;
      if (disposed) return;
      if (!owner) {
        failed = true;
        return;
      }
      controller = createSearchController({
        owner,
        emptyQuery: anchored ? "skip" : "search",
        send: (value, requestId) =>
          options.tabId
            ? commands.newtabSearch(value, { ...owner, request_id: requestId })
            : commands.launcherSearch(value, requestId),
        update: (snapshot) => {
          results = snapshot.results;
          completion = snapshot.completion;
          answered = snapshot.answered;
          pending = snapshot.pending;
          settled = snapshot.settled;
          error = snapshot.error;
          resolveQueued();
        },
      });
      if (query && (!anchored || open)) controller.change(query);
      else if (!anchored) controller.start();
    }

    return () => {
      disposed = true;
      const active = controller?.request();
      if (anchored && active) void commands.newtabCancel(active).catch(() => {});
      controller?.dispose();
      void listener.then((stop) => stop()).catch(() => {});
    };
  }

  return {
    mount,
    changed,
    submit,
    activate,
    move,
    dismiss,
    focused,
    blurred,
    retry,
    applyCompletion,
    hover: (id: string) => {
      moved = true;
      selected = id;
    },
    compositionStart() {
      composing = true;
      controller?.compositionStart();
    },
    compositionEnd(value: string) {
      composing = false;
      query = value;
      controller?.compositionEnd(value);
    },
    setList(element: HTMLElement | undefined) {
      list = element;
    },
    get query() {
      return query;
    },
    /** The query the visible rows answer. Emphasis is measured against this
     *  rather than the live text, so a row's highlight never drops out for the
     *  moment between a keystroke and its answer. */
    get answered() {
      return answered;
    },
    get rows() {
      return rows;
    },
    get selectedId() {
      return selectedId;
    },
    get selectedIndex() {
      return rows.findIndex((row) => row.id === selectedId);
    },
    get open() {
      return open;
    },
    get pending() {
      return pending;
    },
    get running() {
      return running;
    },
    get failed() {
      return failed;
    },
    get error() {
      return error;
    },
    get composing() {
      return composing;
    },
    /** Nothing matched and every native provider has reported. */
    get empty() {
      return !rows.length && !pending && !!query.trim() && !failed && error === "none";
    },
  };
}
