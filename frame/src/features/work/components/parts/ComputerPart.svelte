<script lang="ts">
  import { getContext } from "svelte";
  import type { WorkRuntimeProjection } from "$shared/ipc/bindings";
  import { serviceMark } from "$domain/connections";
  import Icon from "$shared/ui/Icon";
  import Cancel01Icon from "@hugeicons/core-free-icons/Cancel01Icon";
  import File01Icon from "@hugeicons/core-free-icons/File01Icon";
  import FileBracesIcon from "@hugeicons/core-free-icons/FileBracesIcon";
  import FileDiffIcon from "@hugeicons/core-free-icons/FileDiffIcon";
  import FileCodeIcon from "@hugeicons/core-free-icons/FileCodeIcon";
  import FileImageIcon from "@hugeicons/core-free-icons/FileImageIcon";
  import FileTypeIcon from "@hugeicons/core-free-icons/FileTypeIcon";
  import Folder01Icon from "@hugeicons/core-free-icons/Folder01Icon";
  import Tick02Icon from "@hugeicons/core-free-icons/Tick02Icon";
  import * as m from "$shared/i18n/messages";
  import { canvasWork } from "../../lib/canvas-context";
  import { computerView, type ComputerCommand, type ComputerFile } from "../../lib/parts/computer";
  import type { PartContentProps } from "../run/slots";

  let { part, detail, objective, steps }: PartContentProps = $props();

  const work = getContext<((objective: string) => WorkRuntimeProjection | undefined) | undefined>(
    canvasWork,
  );
  const view = $derived(computerView(work?.(objective), steps));
  const FILES = 3;
  /** Three files, or two and a line saying how many more. */
  const shownFiles = $derived(view.files.slice(0, view.files.length > FILES ? FILES - 1 : FILES));
  const moreFiles = $derived(view.files.length - shownFiles.length);
  /** The command that matters now: the running one, else the newest. */
  const shownCommands = $derived(
    [...view.commands]
      .reverse()
      .sort((a, b) => Number(b.state === "running") - Number(a.state === "running"))
      .slice(0, 1),
  );
  const changed = $derived(view.files.filter((file) => file.state === "done").length);
  const running = $derived(view.commands.find((command) => command.state === "running"));

  const CODE =
    /\.(rs|ts|tsx|js|jsx|mjs|svelte|py|go|swift|kt|java|c|cc|cpp|h|hpp|rb|php|cs|vue|sh)$/iu;
  const CONFIG = /\.(json|jsonc|toml|ya?ml|lock|ini|env|xml|plist)$|^(Dockerfile|Makefile)$/iu;
  const DOC = /\.(md|mdx|txt|rst|adoc)$/iu;
  const IMAGE = /\.(png|jpe?g|gif|webp|svg|ico|heic)$/iu;
  function fileIcon(name: string) {
    if (CODE.test(name)) return FileCodeIcon;
    if (CONFIG.test(name)) return FileBracesIcon;
    if (DOC.test(name)) return FileTypeIcon;
    if (IMAGE.test(name)) return FileImageIcon;
    return File01Icon;
  }

  function seconds(ms: number | null): string {
    if (ms == null) return "";
    if (ms < 1000) return m.work_computer_ms({ count: String(ms) });
    if (ms < 60000) return m.work_computer_seconds({ count: (ms / 1000).toFixed(1) });
    return m.work_computer_minutes({
      minutes: String(Math.floor(ms / 60000)),
      seconds: String(Math.floor((ms % 60000) / 1000)).padStart(2, "0"),
    });
  }

  function testsLine(command: ComputerCommand): string | null {
    const tests = command.tests;
    if (!tests) return null;
    return tests.failed
      ? m.work_computer_tests_failed({ failed: String(tests.failed), passed: String(tests.passed) })
      : m.work_computer_tests_passed({ passed: String(tests.passed) });
  }

  function fileNote(file: ComputerFile): string | null {
    switch (file.state) {
      case "waiting":
        return m.work_computer_waiting();
      case "declined":
        return m.work_computer_declined();
      case "failed":
        return m.work_computer_not_applied();
      default:
        return null;
    }
  }

  function commandNote(command: ComputerCommand): string {
    if (command.state === "asking") return m.work_computer_waiting();
    if (command.state === "declined") return m.work_computer_declined();
    if (command.state === "stopped") return m.work_computer_stopped();
    return testsLine(command) ?? seconds(command.ms);
  }

  const reading = $derived.by(() => {
    const parts: string[] = view.folder ? [view.folder] : [];
    if (view.reads)
      parts.push(
        view.reads === 1
          ? m.work_computer_read_one()
          : m.work_computer_read({ count: String(view.reads) }),
      );
    if (view.searches)
      parts.push(
        view.searches === 1
          ? m.work_computer_searched_once()
          : m.work_computer_searched({ count: String(view.searches) }),
      );
    return parts.join(" · ");
  });

  const agentName = { codex: "Codex", claude: "Claude Code" } as const;
  /** A coding agent's newest event: a file it edits (✎ name) or a command it runs. */
  function activity(live: string | null): string {
    if (!live) return m.work_computer_agent_starting();
    return live.startsWith("✎ ")
      ? m.work_computer_agent_editing({ file: live.slice(2) })
      : m.work_computer_agent_running({ command: live.replace(/^\$ /u, "") });
  }
</script>

{#snippet status(state: ComputerCommand["state"] | ComputerFile["state"])}
  <span class="status" data-state={state} aria-hidden="true"
    >{#if state === "passed" || state === "done"}<Icon
        icon={Tick02Icon}
        size={12}
        strokeWidth={2}
      />{:else if state === "failed" || state === "declined"}<Icon
        icon={Cancel01Icon}
        size={11}
        strokeWidth={2}
      />{:else}<i></i>{/if}</span
  >
{/snippet}

<div class="computer {detail}" class:working={view.working} aria-label={part.title}>
  {#if detail === "tile"}
    <div class="tile">
      <Icon icon={FileDiffIcon} size={40} strokeWidth={1.4} />
      {#if changed}<strong>{changed}</strong>{/if}
    </div>
  {:else if detail === "overview"}
    <div class="survey">
      {#if running}<p class="big live">
          {running.agent
            ? m.work_computer_agent_working({ agent: agentName[running.agent] })
            : m.work_computer_agent_running({
                command: running.line.split(/\s+/u).slice(0, 2).join(" "),
              })}
        </p>{:else if changed}<p class="big">
          {changed === 1
            ? m.work_computer_changed_one()
            : m.work_computer_changed({ count: String(changed) })}
        </p>{/if}
      {#if view.tests && !running}<p class="big tests" data-failed={view.tests.failed > 0}>
          <Icon icon={view.tests.failed ? Cancel01Icon : Tick02Icon} size={22} strokeWidth={2} />
          {view.tests.failed
            ? m.work_computer_tests_failing({ count: String(view.tests.failed) })
            : m.work_computer_tests_pass()}
        </p>{/if}
    </div>
  {:else}
    <ul class="rows">
      {#each shownFiles as file (file.key)}
        <li class="file" data-state={file.state} title={file.path}>
          <span class="glyph"><Icon icon={fileIcon(file.name)} size={14} /></span>
          <span class="path"
            >{#if file.folder}<span class="dir">{file.folder}/</span>{/if}<span class="name"
              >{file.name}</span
            ></span
          >
          {#if fileNote(file)}<span class="note">{fileNote(file)}</span>{:else}<span class="counts"
              >{#if file.added}<span class="added">+{file.added}</span>{/if}{#if file.removed}<span
                  class="removed">−{file.removed}</span
                >{/if}</span
            >{/if}
        </li>
      {/each}
      {#if moreFiles}<li class="quiet">
          {moreFiles === 1
            ? m.work_computer_more_file()
            : m.work_computer_more_files({ count: String(moreFiles) })}
        </li>{/if}
      {#each shownCommands as command (command.key)}
        <li class="command" data-state={command.state} title={command.line}>
          {@render status(command.state)}
          {#if command.agent}<span class="agent"
              ><Icon icon={serviceMark(command.agent)} size={13} strokeWidth={1.6} /><strong
                >{agentName[command.agent]}</strong
              ><span class="activity"
                >{command.state === "running"
                  ? activity(command.live)
                  : m.work_computer_agent_worked_short()}</span
              ></span
            >{:else}<code><span class="prompt" aria-hidden="true">$</span>{command.line}</code>{/if}
          <span class="note" class:bad={command.state === "failed" && !!command.tests?.failed}
            >{command.state === "running" ? seconds(command.ms) : commandNote(command)}</span
          >
        </li>
        {#if command.state === "running" && command.live && !command.agent}<li class="live-line">
            <span>{command.live}</span>
          </li>{/if}
      {/each}
      {#if reading && !(running?.live && !running.agent)}<li class="quiet reading">
          <span class="glyph"><Icon icon={Folder01Icon} size={13} /></span>{reading}
        </li>{/if}
    </ul>
  {/if}
</div>

<style>
  .computer {
    box-sizing: border-box;
    inline-size: 100%;
    color: var(--color-text);
  }

  .rows {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
    block-size: 24px;
    font-size: var(--text-label);
    line-height: 16px;
  }

  .glyph {
    display: grid;
    flex: none;
    inline-size: 16px;
    color: var(--color-muted);
    place-items: center;
  }

  .path {
    flex: 1;
    overflow: hidden;
    min-inline-size: 0;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dir {
    color: var(--color-faint);
  }

  .name {
    font-weight: 500;
  }

  .file[data-state="declined"] .name,
  .file[data-state="failed"] .name {
    color: var(--color-muted);
    text-decoration: line-through;
    text-decoration-color: var(--color-faint);
  }

  .counts {
    display: flex;
    flex: none;
    gap: 6px;
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .added {
    color: var(--color-success);
  }

  .removed {
    color: var(--color-danger);
  }

  .note {
    flex: none;
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-variant-numeric: tabular-nums;
  }

  .file[data-state="waiting"] .note,
  .command[data-state="asking"] .note {
    padding: 1px 7px;
    border-radius: var(--radius-capsule);
    background: var(--color-lit-soft);
    color: var(--color-text);
    font-weight: 600;
  }

  .note.bad {
    color: var(--color-danger);
  }

  .command code {
    flex: 1;
    overflow: hidden;
    min-inline-size: 0;
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .prompt {
    margin-inline-end: 6px;
    color: var(--color-faint);
  }

  .agent {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    min-inline-size: 0;
    font-weight: 500;
    white-space: nowrap;
  }

  .agent strong {
    flex: none;
    font-weight: 600;
  }

  .activity {
    overflow: hidden;
    color: var(--color-muted);
    text-overflow: ellipsis;
  }

  .status {
    display: grid;
    flex: none;
    inline-size: 16px;
    block-size: 16px;
    color: var(--color-muted);
    place-items: center;
  }

  .status[data-state="passed"],
  .status[data-state="done"] {
    color: var(--color-success);
  }

  .status[data-state="failed"],
  .status[data-state="declined"] {
    color: var(--color-danger);
  }

  .status i {
    inline-size: 6px;
    block-size: 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-faint);
  }

  .status[data-state="running"] i {
    background: var(--color-accent);
    animation: breathe 1.6s var(--ease-in-out) infinite;
  }

  .status[data-state="asking"] i {
    background: var(--color-warning);
  }

  @keyframes breathe {
    50% {
      opacity: 0.35;
    }
  }

  .live-line {
    block-size: 18px;
    padding-inline-start: 24px;
    margin-block-start: -4px;
  }

  .live-line span {
    overflow: hidden;
    color: var(--color-muted);
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .quiet {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .reading {
    color: var(--color-muted);
  }

  .reading .glyph {
    color: var(--color-faint);
  }

  .survey {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding-block-start: 2px;
  }

  .big {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: var(--text-overview-title);
    font-weight: 600;
    line-height: 1.15;
    letter-spacing: -0.01em;
  }

  .big.live {
    color: var(--color-accent);
  }

  .big.tests {
    color: var(--color-success);
  }

  .big.tests[data-failed="true"] {
    color: var(--color-danger);
  }

  .tile {
    display: flex;
    align-items: center;
    gap: 14px;
    color: var(--color-muted);
  }

  .tile strong {
    color: var(--color-text);
    font-size: var(--text-overview-figure);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .status[data-state="running"] i {
      animation: none;
    }
  }
</style>
