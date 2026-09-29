<script lang="ts">
  import type { WorkflowShape } from "../../lib/start/workflows";

  let { shape }: { shape: WorkflowShape } = $props();
  const rows0 = [
    [26, 46],
    [26, 58],
    [34, 36],
    [34, 50],
    [26, 30],
    [26, 44],
  ] as const;
  const rows1 = [
    [24, 48, 18],
    [84, 30, 22],
    [152, 21, 20],
    [152, 48, 16],
    [152, 75, 18],
  ] as const;
  const rows2 = [
    [20, 86],
    [45, 70],
    [70, 54],
  ] as const;
  const rows3 = [
    [48, 18],
    [72, 26],
    [60, 37],
    [44, 48],
    [66, 59],
    [52, 70],
    [30, 81],
  ] as const;
  const rows4 = [
    [24, 26],
    [96, 20],
    [134, 16],
    [162, 14],
  ] as const;
  const rows5 = [
    ["mint", 32, 104],
    ["lemon", 52, 84],
    ["rose", 72, 58],
  ] as const;
  const rows6 = [
    [52, true, 92],
    [68, true, 70],
    [84, false, 104],
  ] as const;
  const rows7 = [
    [28, false],
    [54, true],
    [80, false],
  ] as const;
  const rows8 = [
    [58, 110],
    [74, 84],
    [90, 96],
  ] as const;
  const spots = [
    [0, 1, 2, 3, 4],
    [0, 1, 2, 3],
    [0, 1, 2],
  ] as const;
</script>

<svg viewBox="0 0 200 100" aria-hidden="true" focusable="false" preserveAspectRatio="xMidYMid meet">
  {#if shape === "flow"}
    <rect class="plate" x="16" y="14" width="86" height="72" rx="7" />
    <rect class="mark" x="20" y="41" width="78" height="14" rx="4" />
    {#each rows0 as [x, w], i (i)}
      <rect class={i === 2 ? "ink" : "t2"} {x} y={22 + i * 10} width={w} height="4" rx="2" />
    {/each}
    <path class="ln" d="M102 48 H118" />
    <rect class="plate" x="118" y="38" width="24" height="20" rx="6" />
    <path class="ln" d="M142 48 H148 V30 H154 M148 48 V66 H154" />
    <rect class="plate" x="154" y="20" width="28" height="20" rx="6" />
    <rect class="hue" x="154" y="56" width="28" height="20" rx="6" />
    <rect class="t2" x="123" y="46" width="14" height="4" rx="2" />
    <rect class="t2" x="160" y="28" width="16" height="4" rx="2" />
  {:else if shape === "map"}
    <path
      class="ln"
      d="M52 50 H65 V32 H78 M65 50 V68 H78 M118 32 H132 V23 H146 M132 32 V50 H146 M118 68 H132 V77 H146"
    />
    <rect class="plate" x="18" y="40" width="34" height="20" rx="6" />
    <rect class="plate" x="78" y="22" width="40" height="20" rx="6" />
    <rect class="hue" x="78" y="58" width="40" height="20" rx="6" />
    <rect class="plate" x="146" y="14" width="38" height="18" rx="6" />
    <rect class="plate" x="146" y="41" width="38" height="18" rx="6" />
    <rect class="gap" x="146.5" y="68.5" width="37" height="17" rx="6" />
    {#each rows1 as [x, y, w], i (i)}
      <rect class="t2" {x} {y} width={w} height="4" rx="2" />
    {/each}
    <rect class="on-hue" x="84" y="66" width="22" height="4" rx="2" />
  {:else if shape === "review"}
    <rect class="plate" x="16" y="12" width="168" height="76" rx="8" />
    <path class="ln" d="M28 37.5 H172 M28 62.5 H172" />
    <circle class="hue" cx="32" cy="25" r="4" />
    <circle class="ring" cx="32" cy="50" r="3.5" />
    <circle class="t2" cx="32" cy="75" r="3" />
    {#each rows2 as [y, w], i (i)}
      <rect class="t1" x="44" {y} width={w} height="5" rx="2.5" />
      <rect class="t2" x="44" y={y + 9} width={w - 26} height="4" rx="2" />
    {/each}
    <rect class="plate-2" x="140" y="18" width="36" height="22" rx="5" />
    <rect class="del" x="144" y="23" width="28" height="5" rx="2" />
    <rect class="add" x="144" y="31" width="28" height="5" rx="2" />
  {:else if shape === "diff"}
    <rect class="plate" x="16" y="12" width="168" height="76" rx="7" />
    <rect class="del" x="16" y="34" width="168" height="11" />
    <rect class="del" x="16" y="45" width="168" height="11" />
    <rect class="add" x="16" y="56" width="168" height="11" />
    <rect class="add" x="16" y="67" width="168" height="11" />
    <path class="sign-del" d="M22 39.5 H27 M22 50.5 H27" />
    <path class="sign-add" d="M22 61.5 H27 M24.5 59 V64 M22 72.5 H27 M24.5 70 V75" />
    {#each rows3 as [w, y], i (i)}
      <rect class="t2" x="36" y={y - 1} width={w} height="4" rx="2" />
    {/each}
  {:else if shape === "screens"}
    <rect class="plate" x="58" y="8" width="126" height="72" rx="7" />
    <path class="ln" d="M58 20.5 H184" />
    {#each [66, 72, 78] as cx (cx)}<circle class="t2" {cx} cy="14.5" r="1.8" />{/each}
    <rect class="t2" x="120" y="30" width="54" height="30" rx="4" />
    <rect class="plate" x="16" y="26" width="112" height="66" rx="7" />
    <path class="ln" d="M16 38.5 H128" />
    {#each [24, 30, 36] as cx (cx)}<circle class="t2" {cx} cy="32.5" r="1.8" />{/each}
    <rect class="t1" x="26" y="48" width="64" height="7" rx="3" />
    <rect class="t2" x="26" y="60" width="48" height="4" rx="2" />
    <rect class="hue" x="26" y="72" width="30" height="11" rx="5.5" />
    <circle class="callout" cx="41" cy="77.5" r="13" />
    <path class="lead" d="M54 77.5 H150" />
    <rect class="hue" x="150" y="71" width="34" height="13" rx="4" />
  {:else if shape === "board"}
    <rect class="tile sky" x="14" y="10" width="52" height="46" rx="5" />
    <circle class="sun" cx="48" cy="24" r="6" />
    <path class="hill" d="M14 56 L32 38 L44 48 L54 40 L66 52 V56 Z" />
    <rect class="tile lemon" x="14" y="60" width="52" height="30" rx="5" />
    <rect class="tile rose" x="72" y="10" width="58" height="32" rx="5" />
    <rect class="tile peach" x="72" y="46" width="58" height="44" rx="5" />
    <rect class="anchor" x="72.75" y="46.75" width="56.5" height="42.5" rx="4.5" />
    <rect class="tile lilac" x="136" y="10" width="50" height="52" rx="5" />
    <rect class="tile mint" x="136" y="66" width="50" height="24" rx="5" />
  {:else if shape === "landscape"}
    <rect class="plate" x="14" y="10" width="172" height="80" rx="7" />
    {#each rows4 as [x, w], i (i)}
      <rect class="t2" {x} y="17" width={w} height="4" rx="2" />
    {/each}
    {#each [0, 1, 2, 3] as row (row)}
      {@const y = 32 + row * 14}
      <path class="ln" d="M22 {y - 3.5} H178" />
      <circle class={row === 0 ? "hue" : "t1"} cx="28" cy={y + 4} r="4.5" />
      <rect class="t1" x="38" y={y + 2} width={[36, 44, 30, 40][row]} height="4" rx="2" />
      <rect class={row === 1 ? "mark" : "none"} x="92" y={y - 1} width="30" height="10" rx="3" />
      <rect
        class={row === 1 ? "ink" : "t2"}
        x="96"
        y={y + 2}
        width={[20, 16, 22, 18][row]}
        height="4"
        rx="2"
      />
      {#if row !== 2}<path class="check" d="M136 {y + 4} l3 3 l6 -6" />{:else}<path
          class="dash"
          d="M136 {y + 4} h9"
        />{/if}
      <rect class="t2" x="162" y={y + 2} width="12" height="4" rx="2" />
    {/each}
  {:else if shape === "campaign"}
    <path class="ln" d="M18 30 H182" />
    {#each [30, 68, 106] as x (x)}
      <circle class="t1" cx={x} cy="30" r="3.5" />
      <rect class="t2" x={x - 10} y="40" width="22" height="4" rx="2" />
    {/each}
    <rect
      class="hue"
      x="145"
      y="24"
      width="12"
      height="12"
      rx="2.5"
      transform="rotate(45 151 30)"
    />
    <rect class="ink" x="140" y="42" width="24" height="4" rx="2" />
    {#each [22, 106] as x (x)}
      <rect class="plate" {x} y="56" width="72" height="32" rx="7" />
      <circle class={x === 22 ? "hue" : "t1"} cx={x + 11} cy="67" r="5" />
      <rect class="t1" x={x + 20} y="64" width="30" height="4" rx="2" />
      <rect class="t2" x={x + 8} y="77" width="54" height="4" rx="2" />
    {/each}
  {:else if shape === "day"}
    {#each [16, 36, 56, 76] as y (y)}
      <rect class="t2" x="14" y={y - 2} width="12" height="4" rx="2" />
      <path class="ln" d="M34 {y} H186" />
    {/each}
    <rect class="plate" x="42" y="19" width="84" height="14" rx="4" />
    <rect class="hue" x="42" y="40" width="136" height="24" rx="5" />
    <rect class="on-hue" x="50" y="46" width="60" height="4" rx="2" />
    <rect class="on-hue" x="50" y="54" width="36" height="4" rx="2" />
    <rect class="plate" x="42" y="70" width="64" height="14" rx="4" />
    <path class="now" d="M34 37 H186" />
    <circle class="now-dot" cx="34" cy="37" r="2.5" />
  {:else if shape === "status"}
    <rect class="hue" x="18" y="12" width="20" height="20" rx="6" />
    <rect class="t1" x="46" y="15" width="42" height="5" rx="2.5" />
    <rect class="t2" x="94" y="15.5" width="18" height="4" rx="2" />
    {#each rows5 as [tone, y, w] (tone)}
      <circle class="dot {tone}" cx="50" cy={y + 4} r="3" />
      <rect class="t1" x="58" y={y + 2} width="30" height="4" rx="2" />
      <rect class="t2" x="58" y={y + 10} width={w} height="4" rx="2" />
    {/each}
  {:else if shape === "weeks"}
    <path class="ln" d="M28 20 H172" />
    {#each [0, 1, 2, 3, 4, 5] as week (week)}
      {@const x = 16 + week * 29}
      <circle class={week < 2 ? "hue" : "ring"} cx={x + 12} cy="20" r="4" />
      {#each [0, 1, 2] as block (block)}
        <rect
          class={week < 2 ? "mark" : "plate"}
          {x}
          y={34 + block * 18}
          width="24"
          height={block === 2 && week % 2 ? 8 : 14}
          rx="4"
        />
      {/each}
    {/each}
  {:else if shape === "exam"}
    {#each [0, 1, 2, 3, 4, 5, 6] as day (day)}
      <rect
        class={day === 6 ? "hue" : "plate"}
        x={16 + day * 24.5}
        y="12"
        width="21"
        height="26"
        rx="5"
      />
      <rect
        class={day === 6 ? "on-hue" : "t2"}
        x={21 + day * 24.5}
        y="28"
        width="11"
        height="4"
        rx="2"
      />
    {/each}
    <path class="on-hue-line" d="M168 16 V24" />
    {#each rows6 as [y, done, w], i (i)}
      <rect class={done ? "hue" : "box"} x="18" y={y - 4} width="9" height="9" rx="2.5" />
      <rect class={done ? "t2" : "t1"} x="34" y={y - 2} width={w} height="4" rx="2" />
    {/each}
  {:else if shape === "inbox"}
    <rect class="plate" x="14" y="10" width="172" height="80" rx="8" />
    <rect class="mark" x="18" y="32" width="164" height="19" rx="5" />
    {#each [0, 1, 2, 3] as row (row)}
      {@const y = 22 + row * 19}
      <circle class={row === 1 ? "hue" : "t1"} cx="30" cy={y} r="5" />
      <rect class="t1" x="42" y={y - 5} width={[40, 52, 34, 46][row]} height="4" rx="2" />
      <rect class="t2" x="42" y={y + 2} width={[86, 70, 92, 60][row]} height="4" rx="2" />
    {/each}
    <rect class="hue" x="148" y="36" width="28" height="11" rx="5.5" />
  {:else if shape === "people"}
    {#each rows7 as [cx, lit], i (i)}
      <circle class={lit ? "hue" : "t1"} {cx} cy="30" r="10" />
    {/each}
    <rect class="plate" x="100" y="17" width="84" height="26" rx="7" />
    <rect class="hue" x="106" y="23" width="3" height="14" rx="1.5" />
    <rect class="t1" x="115" y="23" width="44" height="4" rx="2" />
    <rect class="t2" x="115" y="32" width="30" height="4" rx="2" />
    {#each rows8 as [y, w], i (i)}
      <rect class="box" x="18" y={y - 4} width="9" height="9" rx="2.5" />
      <rect class="t2" x="34" y={y - 2} width={w} height="4" rx="2" />
    {/each}
  {:else if shape === "market"}
    {#each [0, 1, 2, 3] as column (column)}
      {@const x = 14 + column * 44}
      <rect class={column === 1 ? "ink" : "t2"} {x} y="12" width="26" height="4" rx="2" />
      {#if column === 3}<rect
          class="gap"
          x={x + 0.5}
          y="24.5"
          width="37"
          height="63"
          rx="7"
        />{:else}
        {#each spots[column] ?? [] as spot (spot)}
          <circle
            class={column === 1 ? "hue" : "t1"}
            cx={x + 9 + (spot % 2) * 20}
            cy={33 + Math.floor(spot / 2) * 20}
            r="7"
          />
        {/each}
      {/if}
    {/each}
  {:else if shape === "bars"}
    {#each [120, 96, 72, 108, 84] as w, i (i)}
      <rect class="t2" x="16" y={15 + i * 15} width="18" height="4" rx="2" />
      <rect class={i === 2 ? "hue" : "plate"} x="42" y={12 + i * 15} width={w} height="10" rx="4" />
    {/each}
    <path class="median" d="M132 8 V88" />
  {:else}
    {#each [14, 76, 138] as x (x)}
      <rect class="plate" {x} y="10" width="50" height="80" rx="8" />
      <circle class={x === 76 ? "hue" : "t1"} cx={x + 14} cy="26" r="7" />
      <rect class="t1" x={x + 8} y="42" width="32" height="5" rx="2.5" />
      <rect class="t2" x={x + 8} y="52" width="24" height="4" rx="2" />
      <rect class="t2" x={x + 8} y="62" width="30" height="4" rx="2" />
      <rect class={x === 76 ? "ink" : "t2"} x={x + 8} y="76" width="18" height="4" rx="2" />
    {/each}
    <rect class="anchor" x="76.75" y="10.75" width="48.5" height="78.5" rx="7.5" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    inline-size: 100%;
    block-size: 100%;
    overflow: visible;
  }

  .plate {
    fill: var(--color-surface);
    stroke: var(--color-border);
    stroke-width: 1;
  }

  .plate-2 {
    fill: var(--color-raised);
  }

  .t1 {
    fill: var(--color-fill-strong);
  }

  .t2 {
    fill: var(--color-fill-pressed);
  }

  .none {
    fill: none;
  }

  .hue {
    fill: var(--hue);
  }

  .mark {
    fill: var(--hue-edge);
  }

  .ink {
    fill: var(--hue-ink);
  }

  .on-hue {
    fill: var(--color-surface);
    opacity: 0.7;
  }

  .on-hue-line {
    stroke: var(--color-surface);
    stroke-width: 2;
    stroke-linecap: round;
    opacity: 0.8;
  }

  .ring,
  .box {
    fill: none;
    stroke: var(--color-fill-strong);
    stroke-width: 1.5;
  }

  .ln {
    fill: none;
    stroke: var(--color-border-strong);
    stroke-width: 1;
  }

  .gap {
    fill: none;
    stroke: var(--hue-ink);
    stroke-width: 1;
    stroke-dasharray: 3 3;
  }

  .callout {
    fill: none;
    stroke: var(--hue-ink);
    stroke-width: 1.25;
    stroke-dasharray: 2.5 2.5;
  }

  .lead,
  .median {
    fill: none;
    stroke: var(--hue-ink);
    stroke-width: 1;
    stroke-dasharray: 3 3;
  }

  .anchor {
    fill: none;
    stroke: var(--hue-ink);
    stroke-width: 1.5;
  }

  .del {
    fill: var(--color-soft-rose-wash);
  }

  .add {
    fill: var(--color-soft-mint-wash);
  }

  .sign-del,
  .sign-add,
  .check,
  .dash {
    fill: none;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .sign-del {
    stroke: var(--color-soft-rose-ink);
  }

  .sign-add,
  .check {
    stroke: var(--color-soft-mint-ink);
  }

  .dash {
    stroke: var(--color-fill-strong);
  }

  .now {
    fill: none;
    stroke: var(--color-soft-rose);
    stroke-width: 1;
  }

  .now-dot {
    fill: var(--color-soft-rose);
  }

  .tile {
    opacity: 0.62;
  }

  .sky {
    fill: var(--color-soft-sky);
  }

  .mint {
    fill: var(--color-soft-mint);
  }

  .lemon {
    fill: var(--color-soft-lemon);
  }

  .peach {
    fill: var(--color-soft-peach);
  }

  .rose {
    fill: var(--color-soft-rose);
  }

  .lilac {
    fill: var(--color-soft-lilac);
  }

  .sun {
    fill: var(--color-soft-lemon);
  }

  .hill {
    fill: var(--color-soft-mint);
    opacity: 0.7;
  }
</style>
