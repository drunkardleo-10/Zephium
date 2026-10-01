<script lang="ts">
  import {
    ArrowDown01Icon,
    CheckListIcon,
    Clock01Icon,
    HandGrabIcon,
    Mic01Icon,
    PlusSignIcon,
    Search01Icon,
    StickyNote01Icon,
    BookOpen01Icon,
  } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import Icon from "$shared/ui/Icon";
  import { SITES } from "../lib/catalog";
  import LivePage from "./LivePage.svelte";
  import Mark from "./Mark.svelte";

  let { playing }: { playing: boolean } = $props();

  const mark = (id: string) => SITES.find((site) => site.id === id)!.mark;
  const steps = [
    {
      title: m.onb_work_shipped,
      detail: m.onb_work_shipped_detail,
      kind: "issues",
      site: "linear.app",
      mark: mark("linear"),
    },
    {
      title: m.onb_work_merged,
      detail: m.onb_work_merged_detail,
      kind: "review",
      site: "github.com",
      mark: mark("github"),
    },
    {
      title: m.onb_work_discussed,
      detail: m.onb_work_discussed_detail,
      kind: "chat",
      site: "app.slack.com",
      mark: mark("slack"),
    },
  ] as const;
  const table = [
    { label: m.onb_work_row_shipped, values: [14, 9, 3] },
    { label: m.onb_work_row_active, values: [6, 4, 2] },
    { label: m.onb_work_row_blocked, values: [1, 0, 1] },
  ];
  const sources = [
    { name: "Linear", mark: mark("linear") },
    { name: "GitHub", mark: mark("github") },
    { name: "Slack", mark: mark("slack") },
  ];
</script>

<!-- A Work run as the product lays it out: what was asked, one line out to
     what it read, the pages still open and moving, and one line on to what
     it made of them. Plays once from the top when Work comes on screen. -->
<div class="canvas" data-playing={playing}>
  <header>
    <span class="title">{m.onb_work_name()}<Icon icon={ArrowDown01Icon} size={11} /></span>
    <span class="zoom">64%</span>
  </header>

  <div class="context">
    <span
      >{m.onb_work_memory()}<small>{m.onb_work_memory_kind()}</small><i
        ><Icon icon={Clock01Icon} size={9} /></i
      ></span
    >
    <span
      >{m.onb_work_skill()}<small>{m.onb_work_skill_kind()}</small><i
        ><Icon icon={BookOpen01Icon} size={9} /></i
      ></span
    >
  </div>

  <div class="ask">
    <small>{m.onb_work_you()}</small>
    <p>{m.onb_work_prompt()}</p>
  </div>

  <!-- One connection: out of the request, branching to each thing read, and
       gathered again into the result. -->
  <svg class="lines" viewBox="0 0 804 544">
    <path
      class="trunk"
      d="M302 254 H316 M316 130 V378 M316 130 Q316 120 326 120 H338 M316 254 H338 M316 378 Q316 388 326 388 H338 M436 120 H446 M436 254 H446 M436 388 H446"
    />
    <path
      class="gather"
      d="M592 120 H604 Q614 120 614 130 V378 Q614 388 604 388 H592 M592 254 H632"
    />
  </svg>

  {#each steps as step, index (index)}
    <div class="step" style:--at={`${120 + index * 134}px`} style:--i={index}>
      <b><Icon icon={Search01Icon} size={9} />{step.title()}</b>
      <span>{step.detail()}</span>
    </div>
    <div class="live" style:--at={`${120 + index * 134}px`} style:--i={index}>
      <span class="done"><i></i>{m.onb_work_done()}</span>
      <LivePage kind={step.kind} site={step.site} mark={step.mark} />
    </div>
  {/each}

  <section class="result">
    <h3>{m.onb_work_result()}</h3>
    <p>{m.onb_work_result_body()}</p>
    <ul>
      <li>{m.onb_work_point_one()}</li>
      <li>{m.onb_work_point_two()}</li>
    </ul>
    <div class="table">
      <span></span>
      {#each sources as source (source.name)}<span class="source"
          ><Mark mark={source.mark} size={10} />{source.name}</span
        >{/each}
      {#each table as row, r (r)}
        <span class="key">{row.label()}</span>
        {#each row.values as value, c (c)}<span class="value" style:--d={r * 3 + c}>{value}</span
          >{/each}
      {/each}
    </div>
  </section>

  <div class="composer">
    <Icon icon={HandGrabIcon} size={13} />
    <Icon icon={StickyNote01Icon} size={13} />
    <Icon icon={CheckListIcon} size={13} />
    <Icon icon={PlusSignIcon} size={13} />
    <i class="rule"></i>
    <span>{m.onb_work_composer()}</span>
    <Icon icon={Mic01Icon} size={13} />
  </div>
</div>

<style>
  .canvas {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(circle, var(--color-border-strong) 0.8px, transparent 1.1px) 0 0 / 14px 14px,
      var(--color-page);
    font-size: 10px;
  }

  .canvas:not([data-playing="true"]) * {
    animation-play-state: paused !important;
  }

  header {
    position: absolute;
    inset: 16px 22px auto;
    display: flex;
    justify-content: space-between;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 15px;
    font-weight: 500;
    letter-spacing: -0.01em;
  }

  .title :global(svg),
  .zoom {
    color: var(--color-muted);
  }

  .zoom {
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }

  .context {
    position: absolute;
    inset-block-start: 234px;
    inset-inline-start: 16px;
    display: grid;
    gap: 9px;
    text-align: end;
    animation: rise 600ms var(--ease-emphasized) 200ms backwards;
  }

  .context span {
    display: grid;
    grid-template-columns: auto 16px;
    column-gap: 6px;
    align-items: center;
    font-size: 8.5px;
  }

  .context small {
    grid-row: 2;
    color: var(--color-faint);
    font-size: 7.5px;
  }

  .context i {
    display: grid;
    grid-column: 2;
    grid-row: 1 / 3;
    place-items: center;
    block-size: 16px;
    border-radius: var(--radius-inset);
    background: var(--color-fill-strong);
    color: var(--color-muted);
  }

  .ask {
    position: absolute;
    inset-block-start: 236px;
    inset-inline-start: 130px;
    inline-size: 166px;
    animation: rise 600ms var(--ease-emphasized) 400ms backwards;
  }

  .ask small {
    color: var(--color-faint);
    font-size: 8.5px;
  }

  .ask p {
    margin: 4px 0 0;
    font-size: 11px;
    line-height: 1.4;
  }

  .lines {
    position: absolute;
    inset: 0;
    inline-size: 100%;
    block-size: 100%;
    overflow: visible;
  }

  .lines path {
    fill: none;
    stroke: var(--color-border-strong);
    stroke-width: 1;
    stroke-dasharray: 700;
    stroke-dashoffset: 700;
    animation: draw 1100ms var(--ease-in-out) forwards;
  }

  .lines .trunk {
    animation-delay: 800ms;
  }

  .lines .gather {
    animation-delay: 3100ms;
  }

  .step {
    position: absolute;
    inset-block-start: calc(var(--at) - 13px);
    inset-inline-start: 342px;
    inline-size: 94px;
    animation: rise 520ms var(--ease-emphasized) backwards;
    animation-delay: calc(1300ms + var(--i) * 180ms);
  }

  .step b {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 9.5px;
    font-weight: 600;
  }

  .step b :global(svg) {
    color: var(--color-faint);
  }

  .step span {
    display: block;
    margin-block-start: 3px;
    padding-inline-start: 14px;
    color: var(--color-faint);
    font-size: 8px;
    line-height: 1.35;
  }

  .live {
    position: absolute;
    inset-block-start: calc(var(--at) - 44px);
    inset-inline-start: 448px;
    inline-size: 136px;
    block-size: 88px;
    animation: rise 620ms var(--ease-emphasized) backwards;
    animation-delay: calc(1500ms + var(--i) * 180ms);
  }

  .done {
    position: absolute;
    inset-block-end: calc(100% + 4px);
    inset-inline-end: 2px;
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--color-faint);
    font-size: 7.5px;
    animation: rise 400ms var(--ease-out) backwards;
    animation-delay: calc(2500ms + var(--i) * 260ms);
  }

  .done i {
    inline-size: 6px;
    block-size: 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-success);
  }

  .result {
    position: absolute;
    inset-block-start: 112px;
    inset-inline: 638px 18px;
    display: grid;
    gap: 7px;
    animation: rise 700ms var(--ease-emphasized) 3500ms backwards;
  }

  .result h3 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    line-height: 1.25;
    letter-spacing: -0.01em;
  }

  .result p,
  .result ul {
    margin: 0;
    color: var(--color-muted);
    font-size: 8.5px;
    line-height: 1.5;
  }

  .result ul {
    padding-inline-start: 10px;
  }

  .table {
    display: grid;
    grid-template-columns: 1.3fr repeat(3, 1fr);
    gap: 7px 2px;
    align-items: center;
    margin-block-start: 4px;
    padding: 9px;
    border-radius: var(--radius-control-compact);
    background: var(--color-card);
    box-shadow: inset 0 0 0 0.5px var(--color-border);
    font-size: 8px;
  }

  .source {
    display: grid;
    justify-items: center;
    gap: 3px;
    font-weight: 600;
  }

  .key {
    color: var(--color-muted);
  }

  .value {
    justify-self: center;
    font-variant-numeric: tabular-nums;
    animation: pop 420ms var(--ease-snap) backwards;
    animation-delay: calc(3900ms + var(--d) * 60ms);
  }

  .composer {
    position: absolute;
    inset-block-end: 14px;
    inset-inline-start: 50%;
    display: flex;
    align-items: center;
    gap: 12px;
    inline-size: 430px;
    block-size: 38px;
    padding-inline: 14px;
    box-sizing: border-box;
    transform: translateX(-50%);
    border-radius: var(--radius-row);
    background: var(--color-card);
    box-shadow:
      var(--shadow-raised),
      inset 0 0 0 0.5px var(--color-border);
    color: var(--color-muted);
  }

  .composer .rule {
    inline-size: 1px;
    block-size: 18px;
    background: var(--color-border);
  }

  .composer span {
    flex: 1;
    color: var(--color-faint);
    font-size: 11px;
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  @keyframes draw {
    to {
      stroke-dashoffset: 0;
    }
  }

  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.4);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .canvas * {
      animation-duration: 1ms !important;
      animation-delay: 0ms !important;
    }
  }
</style>
