<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import type { Detail, DraftView, ObjectActions } from "../../lib/board/types";
  import Inline from "./Inline.svelte";
  import Mark from "./Mark.svelte";
  import { blocks } from "./markdown";
  import { Tick02Icon } from "./icons";
  /**
   * A message in the shape of where it goes: a Slack message under its channel,
   * an email with its To and Subject, a post with its author, a comment on its
   * issue. Send goes through Confirm; it never sends by itself.
   */
  let {
    object,
    detail,
    actions = {},
    centre = false,
  }: {
    object: DraftView;
    detail: Detail;
    actions?: ObjectActions;
    /** Opened in the centre: the words can be changed before they go. */
    centre?: boolean;
  } = $props();
  // What the person is writing, reset whenever the draft itself changes.
  let text = $derived(object.body);
  const HOST = {
    slack: "slack.com",
    email: "mail.google.com",
    linkedin: "linkedin.com",
    x: "x.com",
    github: "github.com",
    message: "",
  } as const;
  const host = $derived(HOST[object.destination]);
  const send = $derived(object.send ?? "draft");
  const verb = $derived(
    object.destination === "linkedin" || object.destination === "x"
      ? m.work_draft_post()
      : object.destination === "github"
        ? m.work_draft_comment()
        : m.work_draft_send(),
  );
  const where = $derived(
    object.destination === "slack"
      ? "Slack"
      : object.destination === "email"
        ? m.work_draft_email()
        : object.destination === "linkedin"
          ? "LinkedIn"
          : object.destination === "x"
            ? "X"
            : object.destination === "github"
              ? "GitHub"
              : m.work_draft_message(),
  );
  const parts = $derived(blocks(object.body));
  const gist = $derived(
    object.subject ??
      (parts[0]?.kind === "paragraph" ? parts[0].lines[0] : parts[0]?.items[0]) ??
      "",
  );
  /** X counts what it lets through; past its limit the count turns. */
  const left = $derived(object.destination === "x" ? 280 - [...object.body].length : null);
</script>

{#snippet body()}
  {#if centre && actions.write}<textarea
      class="body edit nodrag nopan nowheel"
      aria-label={where}
      bind:value={text}
      onblur={() => text !== object.body && actions.write?.(object.id, text)}></textarea>{:else}
    <div class="body">
      {#each parts as part, index (index)}
        {#if part.kind === "list"}<ul>
            {#each part.items as item, at (at)}<li><Inline text={item} /></li>{/each}
          </ul>{:else}<p>
            {#each part.lines as line, at (at)}{#if at}<br />{/if}<Inline text={line} />{/each}
          </p>{/if}
      {/each}
    </div>{/if}
{/snippet}

{#snippet author(size: number)}
  {#if object.author?.picture}<img
      class="avatar"
      src={object.author.picture.src}
      alt=""
      width={size}
      height={size}
      decoding="async"
    />{/if}
{/snippet}

<article class="draft {object.destination} {detail}" aria-label={where}>
  <header>
    {#if host}<Mark address={host} size={detail === "full" ? 16 : 32} />{/if}
    <span class="where">{where}</span>
    {#if object.to && object.destination !== "email"}<span class="to">{object.to}</span>{/if}
    {#if detail === "full"}<span class="state">{m.work_draft_draft()}</span>{/if}
  </header>
  {#if detail === "full"}
    {#if object.destination === "email"}
      <dl class="fields">
        {#if object.to}<div>
            <dt>{m.work_draft_to()}</dt>
            <dd>{object.to}</dd>
          </div>{/if}
        {#if object.subject}<div>
            <dt>{m.work_draft_subject()}</dt>
            <dd class="subject">{object.subject}</dd>
          </div>{/if}
      </dl>
      {@render body()}
    {:else if object.destination === "slack" || object.destination === "message"}
      <div class="message">
        {@render author(32)}
        <div class="said">
          {#if object.author}<p class="who">
              <strong>{object.author.name}</strong><span class="when">{m.work_draft_now()}</span>
            </p>{/if}
          {@render body()}
        </div>
      </div>
    {:else if object.destination === "linkedin" || object.destination === "x"}
      {#if object.author}<div class="byline">
          {@render author(40)}
          <div class="names">
            <strong>{object.author.name}</strong>
            {#if object.author.handle}<span class="handle">{object.author.handle}</span>{/if}
          </div>
        </div>{/if}
      {@render body()}
    {:else}
      <div class="comment">
        {#if object.author}<p class="who"><strong>{object.author.name}</strong></p>{/if}
        {@render body()}
      </div>
    {/if}
    <footer>
      {#if left !== null}<span class="count" class:over={left < 0}>{left}</span>{/if}
      <button
        type="button"
        class="send nodrag nopan"
        disabled={send !== "draft" || (left !== null && left < 0)}
        onclick={() => actions.send?.(object.id)}
        >{#if send === "sent"}<Icon
            icon={Tick02Icon}
            size={13}
          />{m.work_draft_sent()}{:else if send === "confirming"}{m.work_draft_confirming()}{:else}{verb}{/if}</button
      >
    </footer>
  {:else}
    <p class="gist"><Inline text={gist} /></p>
  {/if}
</article>

<style>
  .draft {
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-sizing: border-box;
    inline-size: 100%;
    padding: 14px 18px 16px;
    border-radius: var(--radius-card);
    background: var(--color-surface);
    box-shadow: var(--shadow-raised);
    color: var(--color-text);
  }

  header {
    display: flex;
    align-items: center;
    gap: 7px;
    min-inline-size: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .where {
    color: var(--color-label-secondary);
    font-weight: 600;
  }

  .to {
    min-inline-size: 0;
    overflow-wrap: anywhere;
  }

  .to::before {
    margin-inline-end: 7px;
    color: var(--color-faint);
    content: "·";
  }

  .state {
    margin-inline-start: auto;
    padding: 1px 8px;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-muted);
    font-size: var(--text-caption);
    font-weight: 500;
  }

  .fields {
    display: flex;
    flex-direction: column;
    margin: 0;
    font-size: var(--text-body);
  }

  .fields div {
    display: flex;
    gap: 10px;
    padding-block: 7px;
    border-block-end: 1px solid var(--color-border);
  }

  .fields div:first-child {
    border-block-start: 1px solid var(--color-border);
  }

  dt {
    flex: none;
    inline-size: 56px;
    color: var(--color-muted);
  }

  dd {
    margin: 0;
    min-inline-size: 0;
    overflow-wrap: anywhere;
  }

  .subject {
    font-weight: 600;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: var(--text-page-title);
    line-height: 1.5;
    text-wrap: pretty;
  }

  .edit {
    box-sizing: border-box;
    inline-size: 100%;
    min-block-size: 220px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-page-title);
    line-height: 1.5;
    resize: vertical;
    outline: none;
  }

  .body p,
  .body ul {
    margin: 0;
  }

  .body ul {
    padding-inline-start: 20px;
  }

  .message {
    display: flex;
    gap: 10px;
  }

  .avatar {
    flex: none;
    border-radius: var(--radius-inset);
    object-fit: cover;
  }

  .linkedin .avatar,
  .x .avatar {
    border-radius: var(--radius-capsule);
  }

  .said {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-inline-size: 0;
  }

  .who {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0;
    font-size: var(--text-body);
  }

  .when {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .byline {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .names {
    display: flex;
    flex-direction: column;
    font-size: var(--text-body);
  }

  .handle {
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .comment {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border-radius: var(--radius-row);
    box-shadow: inset 0 0 0 1px var(--color-border-strong);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
  }

  .count {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-variant-numeric: tabular-nums;
  }

  .count.over {
    color: var(--color-danger);
  }

  .send {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    block-size: 28px;
    padding: 0 14px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 600;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .send:disabled {
    background: var(--color-fill);
    color: var(--color-muted);
  }

  .send:hover:not(:disabled) {
    background: var(--color-lit-hover);
  }

  .gist {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    font-size: var(--text-overview-label);
    line-height: 1.35;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }

  .overview,
  .tile {
    gap: 18px;
    padding: 24px 28px;
  }

  .overview header,
  .tile header {
    gap: 12px;
    font-size: var(--text-overview-label);
  }

  .tile .gist {
    display: none;
  }

  .tile .where {
    font-size: var(--text-tile-title);
  }

  .tile .to {
    display: none;
  }
</style>
